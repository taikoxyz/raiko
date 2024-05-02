use raiko_pipeline::Pipeline;
fn main() {
    let pipeline = raiko_pipeline::sp1::Sp1Pipeline::new("../guest", "release");
    pipeline.bins(&["sp1-guest"], "../guest/elf");
    pipeline.tests(&["sp1-guest"], "../guest/elf");
}
