use zeth_primitives::transactions::ethereum::EthereumTxEssence;

use crate::{
    block_builder::{ConfiguredBlockBuilder, NetworkStrategyBundle},
    finalization::BuildFromMemDbStrategy,
    initialization::MemDbInitStrategy,
    mem_db::MemDb,
    taiko::prepare::TaikoHeaderPrepStrategy,
};
use crate::taiko::execute::TaikoTxExecStrategy;

pub struct TaikoStrategyBundle {}

impl NetworkStrategyBundle for TaikoStrategyBundle {
    type Database = MemDb;
    type TxEssence = EthereumTxEssence;
    type DbInitStrategy = MemDbInitStrategy;
    type HeaderPrepStrategy = TaikoHeaderPrepStrategy;
    type TxExecStrategy = TaikoTxExecStrategy;
    type BlockBuildStrategy = BuildFromMemDbStrategy;
}

pub type TaikoBlockBuilder<'a> = ConfiguredBlockBuilder<'a, TaikoStrategyBundle>;
