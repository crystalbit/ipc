// Copyright 2022-2024 Protocol Labs
// SPDX-License-Identifier: MIT
//! Bottom up checkpoint manager

pub mod monitor;

use crate::checkpoint::monitor::{
    ensure_monitoring_setup, BU_CHECKPOINT_ACTUAL_GAS, BU_CHECKPOINT_GAS_ESTIMATED,
    BU_CHECKPOINT_GAS_PREMIUM, BU_CHECKPOINT_GAS_PRICE, LATEST_COMMITTED_BU_HEIGHT,
    NUM_BU_CHECKPOINT_FAILED, NUM_BU_CHECKPOINT_SUBMITTED, NUM_BU_CHECKPOINT_SUCCEEDED,
};
use crate::config::Subnet;
use crate::manager::{BottomUpCheckpointRelayer, EthSubnetManager};
use anyhow::{anyhow, Result};
use fvm_shared::address::Address;
use fvm_shared::clock::ChainEpoch;
use fvm_shared::econ::TokenAmount;
use ipc_wallet::{EthKeyAddress, PersistentKeyStore};
use num_traits::ToPrimitive;
use std::cmp::max;
use std::fmt::{Display, Formatter};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Tracks the config required for bottom up checkpoint submissions
/// parent/child subnet and checkpoint period.
pub struct CheckpointConfig {
    parent: Subnet,
    child: Subnet,
    period: ChainEpoch,
}

/// Manages the submission of bottom up checkpoint. It checks if the submitter has already
/// submitted in the `last_checkpoint_height`, if not, it will submit the checkpoint at that height.
/// Then it will submit at the next submission height for the new checkpoint.
pub struct BottomUpCheckpointManager<T> {
    metadata: CheckpointConfig,
    parent_handler: T,
    child_handler: T,
    /// The number of blocks away from the chain head that is considered final
    finalization_blocks: ChainEpoch,
}

impl<T: BottomUpCheckpointRelayer> BottomUpCheckpointManager<T> {
    pub async fn new(
        parent: Subnet,
        child: Subnet,
        parent_handler: T,
        child_handler: T,
    ) -> Result<Self> {
        let period = parent_handler
            .checkpoint_period(&child.id)
            .await
            .map_err(|e| anyhow!("cannot get bottom up checkpoint period: {e}"))?;
        Ok(Self {
            metadata: CheckpointConfig {
                parent,
                child,
                period,
            },
            parent_handler,
            child_handler,
            finalization_blocks: 0,
        })
    }

    pub fn with_finalization_blocks(mut self, finalization_blocks: ChainEpoch) -> Self {
        self.finalization_blocks = finalization_blocks;
        self
    }
}

impl BottomUpCheckpointManager<EthSubnetManager> {
    pub async fn new_evm_manager(
        parent: Subnet,
        child: Subnet,
        keystore: Arc<RwLock<PersistentKeyStore<EthKeyAddress>>>,
    ) -> Result<Self> {
        let parent_handler =
            EthSubnetManager::from_subnet_with_wallet_store(&parent, Some(keystore.clone()))?;
        let child_handler =
            EthSubnetManager::from_subnet_with_wallet_store(&child, Some(keystore))?;
        Self::new(parent, child, parent_handler, child_handler).await
    }
}

impl<T: BottomUpCheckpointRelayer> Display for BottomUpCheckpointManager<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "bottom-up relayer, parent: {:}, child: {:}",
            self.metadata.parent.id, self.metadata.child.id
        )
    }
}

impl<T: BottomUpCheckpointRelayer + Send + Sync + 'static> BottomUpCheckpointManager<T> {
    /// Getter for the parent subnet this checkpoint manager is handling
    pub fn parent_subnet(&self) -> &Subnet {
        &self.metadata.parent
    }

    /// Getter for the target subnet this checkpoint manager is handling
    pub fn child_subnet(&self) -> &Subnet {
        &self.metadata.child
    }

    /// The checkpoint period that the current manager is submitting upon
    pub fn checkpoint_period(&self) -> ChainEpoch {
        self.metadata.period
    }

    /// Run the bottom up checkpoint submission daemon in the foreground
    pub async fn run(self, submitter: Address, submission_interval: Duration) -> Result<()> {
        log::info!("launching {self} for {submitter}");

        ensure_monitoring_setup()?;

        loop {
            self.submit_checkpoint(&submitter).await;
            tokio::time::sleep(submission_interval).await;
        }
    }

    /// Submit the checkpoint from the target submitter address
    pub async fn submit_checkpoint(&self, submitter: &Address) {
        let next_submission_height = if let Ok(h) = self.next_submission_height().await {
            h
        } else {
            log::error!("cannot fetch next submission height for submitter {submitter}");
            return;
        };

        if let Err(e) = self.submit_epoch(next_submission_height, submitter).await {
            log::error!(
                "cannot submit at bottom up checkpoint for height: {} and submitter {} due to {}",
                next_submission_height,
                submitter,
                e
            )
        }
    }

    /// Derive the next submission checkpoint height
    async fn next_submission_height(&self) -> Result<ChainEpoch> {
        let last_checkpoint_epoch = self
            .parent_handler
            .last_bottom_up_checkpoint_height(&self.metadata.child.id)
            .await
            .map_err(|e| {
                anyhow!("cannot obtain the last bottom up checkpoint height due to: {e:}")
            })?;
        Ok(last_checkpoint_epoch + self.checkpoint_period())
    }

    /// Checks if the relayer has already submitted at the next submission epoch, if not it submits it.
    async fn submit_epoch(
        &self,
        next_submission_height: ChainEpoch,
        submitter: &Address,
    ) -> Result<()> {
        let current_height = self.child_handler.current_epoch().await?;
        let finalized_height = max(1, current_height - self.finalization_blocks);

        log::debug!("next_submission_height: {next_submission_height}, current height: {current_height}, finalized_height: {finalized_height}");

        if finalized_height < next_submission_height {
            return Ok(());
        }

        let prev_h = next_submission_height - self.checkpoint_period();
        log::debug!("start querying quorum reached events from : {prev_h} to {finalized_height}");

        for h in (prev_h + 1)..=finalized_height {
            let events = self.child_handler.quorum_reached_events(h).await?;
            if events.is_empty() {
                log::debug!("no reached events at height : {h}");
                continue;
            }

            log::debug!("found reached events at height : {h}");

            for event in events {
                let bundle = self
                    .child_handler
                    .checkpoint_bundle_at(event.height)
                    .await?;
                log::debug!("bottom up bundle: {bundle:?}");

                NUM_BU_CHECKPOINT_SUBMITTED.inc();

                let txn_detail = self
                    .parent_handler
                    .submit_checkpoint(
                        submitter,
                        bundle.checkpoint,
                        bundle.signatures,
                        bundle.signatories,
                    )
                    .await
                    .map_err(|e| {
                        NUM_BU_CHECKPOINT_FAILED.inc();
                        anyhow!("cannot submit bottom up checkpoint due to: {e:}")
                    })?;

                process_if_f64(&txn_detail.estimated_gas, |a| {
                    BU_CHECKPOINT_GAS_ESTIMATED.observe(a)
                });
                process_if_f64(&txn_detail.actual_gas, |a| {
                    BU_CHECKPOINT_ACTUAL_GAS.observe(a)
                });
                process_some_if_f64(&txn_detail.gas_premium, |a| {
                    BU_CHECKPOINT_GAS_PREMIUM.observe(a)
                });
                process_some_if_f64(&txn_detail.gas_price, |a| {
                    BU_CHECKPOINT_GAS_PRICE.observe(a)
                });

                LATEST_COMMITTED_BU_HEIGHT.set(event.height);
                NUM_BU_CHECKPOINT_SUCCEEDED.inc();

                log::info!(
                    "submitted bottom up checkpoint({}) in parent at height {}",
                    event.height,
                    txn_detail.payload
                );
            }
        }

        Ok(())
    }
}

/// Call the function f if amount can be parsed to f64
fn process_if_f64<F: FnOnce(f64)>(amount: &TokenAmount, f: F) {
    if let Some(amt) = amount.atto().to_f64() {
        f(amt)
    }
}

/// Call the function f if amount is not optional and can be parsed to f64
fn process_some_if_f64<F: FnOnce(f64)>(amount: &Option<TokenAmount>, f: F) {
    if let Some(amt) = amount.as_ref() {
        process_if_f64(amt, f);
    }
}
