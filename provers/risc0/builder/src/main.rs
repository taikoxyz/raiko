use raiko_pipeline::Pipeline;
fn main() {
    let pipeline = raiko_pipeline::risc0::Risc0Pipeline::new("../guest", "release");
    pipeline.bins(&["risc0-guest"], "../driver/src/methods");
    pipeline.tests(&["risc0-guest"], "../driver/src/methodsf");
}
