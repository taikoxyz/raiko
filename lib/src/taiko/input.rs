use serde::{Deserialize, Serialize};
use zeth_primitives::transactions::TxEssence;

use crate::{
    input::Input,
    taiko::host::{TaikoExtra, TaikoInit},
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TaikoInput<E: TxEssence> {
    pub l1_input: Input<E>,
    pub l2_input: Input<E>,
    pub extra: TaikoExtra,
}

impl<E: TxEssence> From<TaikoInit<E>> for TaikoInput<E> {
    fn from(value: TaikoInit<E>) -> TaikoInput<E> {
        TaikoInput {
            l1_input: value.l1_init.into(),
            l2_input: value.l2_init.into(),
            extra: value.extra,
        }
    }
}
