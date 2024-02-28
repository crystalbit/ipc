// Copyright 2022-2024 Protocol Labs
// SPDX-License-Identifier: MIT

//! Bottom up checkpoint monitoring with prometheus

use anyhow::anyhow;
use lazy_static::lazy_static;
use prometheus::{Histogram, HistogramOpts, IntCounter, IntGauge, Registry};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

lazy_static! {
    static ref IS_SETUP_BU_CHECKPOINT_MONITORING: AtomicBool = AtomicBool::new(false);
    pub(crate) static ref LATEST_COMMITTED_BU_HEIGHT: IntGauge = IntGauge::new(
        "latest_committed_bu_height",
        "Latest bottom up checkpoint height committed to the parent"
    )
    .expect("latest_committed_bu_height can be created");
    pub(crate) static ref NUM_BU_CHECKPOINT_SUBMITTED: IntCounter = IntCounter::new(
        "num_bu_checkpoint_submitted",
        "Number of buttom up checkpoint submitted"
    )
    .expect("num_bu_checkpoint_submitted can be created");
    pub(crate) static ref NUM_BU_CHECKPOINT_SUCCEEDED: IntCounter = IntCounter::new(
        "num_bu_checkpoint_succeeded",
        "Number of buttom up checkpoint successfully submitted"
    )
    .expect("num_bu_checkpoint_succeeded can be created");
    pub(crate) static ref NUM_BU_CHECKPOINT_FAILED: IntCounter = IntCounter::new(
        "num_bu_checkpoint_failed",
        "Number of buttom up checkpoint submission failed"
    )
    .expect("num_bu_checkpoint_failed can be created");
    pub(crate) static ref BU_CHECKPOINT_GAS_ESTIMATED: Histogram =
        Histogram::with_opts(HistogramOpts::new(
            "bu_checkpoint_gas_estimated",
            "Gas estimated for bottom up checkpoint submission"
        ),)
        .expect("bu_checkpoint_gas_estimated can be created");
    pub(crate) static ref BU_CHECKPOINT_ACTUAL_GAS: Histogram =
        Histogram::with_opts(HistogramOpts::new(
            "bu_checkpoint_actual_gas",
            "Actual gas used for bottom up checkpoint submission"
        ),)
        .expect("bu_checkpoint_actual_gas can be created");
    pub(crate) static ref BU_CHECKPOINT_GAS_PREMIUM: Histogram =
        Histogram::with_opts(HistogramOpts::new(
            "bu_checkpoint_gas_premium",
            "Gas premium for bottom up checkpoint submission"
        ),)
        .expect("bu_checkpoint_gas_premium can be created");
    pub(crate) static ref BU_CHECKPOINT_GAS_PRICE: Histogram =
        Histogram::with_opts(HistogramOpts::new(
            "bu_checkpoint_gas_price",
            "Gas price for bottom up checkpoint submission"
        ),)
        .expect("bu_checkpoint_gas_price can be created");
}

/// Setup prometheus registry and metrics, call this function before BottomUpCheckpointManager is
/// run.
///
/// # Arguments
///
/// * prefix The prefix namespace attached to the metrics
/// * labels The labels attached to the metrics, such as subnet_id -> /r123/.... and other metadata
pub fn setup(prefix: String, labels: HashMap<String, String>) -> anyhow::Result<()> {
    let registry = registry(prefix, labels)?;

    registry.register(Box::new(LATEST_COMMITTED_BU_HEIGHT.clone()))?;
    registry.register(Box::new(NUM_BU_CHECKPOINT_SUBMITTED.clone()))?;
    registry.register(Box::new(NUM_BU_CHECKPOINT_SUCCEEDED.clone()))?;
    registry.register(Box::new(NUM_BU_CHECKPOINT_FAILED.clone()))?;
    registry.register(Box::new(BU_CHECKPOINT_GAS_ESTIMATED.clone()))?;
    registry.register(Box::new(BU_CHECKPOINT_ACTUAL_GAS.clone()))?;
    registry.register(Box::new(BU_CHECKPOINT_GAS_PREMIUM.clone()))?;
    registry.register(Box::new(BU_CHECKPOINT_GAS_PRICE.clone()))?;

    IS_SETUP_BU_CHECKPOINT_MONITORING.store(true, Ordering::SeqCst);

    Ok(())
}

pub(crate) fn ensure_monitoring_setup() -> anyhow::Result<()> {
    if IS_SETUP_BU_CHECKPOINT_MONITORING.load(Ordering::SeqCst) {
        Ok(())
    } else {
        Err(anyhow!(
            "bottom up checkpoint monitoring has yet to be setup"
        ))
    }
}

fn registry(prefix: String, labels: HashMap<String, String>) -> anyhow::Result<Registry> {
    Ok(Registry::new_custom(Some(prefix), Some(labels))?)
}
