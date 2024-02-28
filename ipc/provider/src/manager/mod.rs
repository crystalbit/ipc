// Copyright 2022-2024 Protocol Labs
// SPDX-License-Identifier: MIT
pub use crate::lotus::message::ipc::SubnetInfo;
pub use evm::{EthManager, EthSubnetManager};
use fvm_shared::econ::TokenAmount;
pub use subnet::{
    BottomUpCheckpointRelayer, GetBlockHashResult, SubnetGenesisInfo, SubnetManager,
    TopDownFinalityQuery, TopDownQueryPayload,
};

pub mod evm;
mod subnet;

/// Contains the detailed information of the txn call
pub struct TransactionDetail<T> {
    /// The execution result of the txn
    pub payload: T,
    /// The estimated gas before the txn was executed
    pub estimated_gas: TokenAmount,
    /// The estimated gas before the txn was executed
    pub actual_gas: TokenAmount,
    /// The actual gas price used
    pub gas_price: Option<TokenAmount>,
    /// The actual gas premium used
    pub gas_premium: Option<TokenAmount>,
}
