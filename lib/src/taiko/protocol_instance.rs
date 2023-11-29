use anyhow::Result;
use zeth_primitives::{block::Header, taiko::ProtocolInstance, transactions::TxEssence};

use crate::taiko::input::TaikoInput;

pub fn assemble_protocol_instance<E: TxEssence>(
    input: &TaikoInput<E>,
    header: &Header,
) -> Result<ProtocolInstance> {
    todo!()
}
