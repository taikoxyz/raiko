use serde::{Deserialize, Serialize};
use zeth_primitives::{transactions::TxEssence, B256};

use crate::{input::Input, taiko::host::TaikoInit};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TaikoInput<E: TxEssence> {
    pub l1_input: Input<E>,
    pub l2_input: Input<E>,
    pub tx_list: Vec<u8>,
    pub l1_hash: B256,
    pub l1_height: u64,
}

impl<E: TxEssence> From<TaikoInit<E>> for TaikoInput<E> {
    fn from(value: TaikoInit<E>) -> TaikoInput<E> {
        TaikoInput {
            l1_input: value.l1_init.into(),
            l2_input: value.l2_init.into(),
            tx_list: value.tx_list,
            l1_hash: value.l1_hash,
            l1_height: value.l1_height,
        }
    }
}
