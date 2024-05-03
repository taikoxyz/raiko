use raiko_pipeline::Pipeline;
fn main() {
    let pipeline = raiko_pipeline::risc0::Risc0Pipeline::new("provers/risc0/guest", "release");
    pipeline.bins(&["risc0-guest"], "provers/risc0/driver/src/methods");
    pipeline.tests(&["risc0-guest"], "provers/risc0/driver/src/methods");
}
