



use zeth_lib::input::{GuestInput, GuestOutput};



pub async fn execute_sp1(
    input: GuestInput,
    _output: GuestOutput,
) -> Result<sp1_guest::Sp1Response, String> {
    sp1_guest::execute(input).await
}
