use raiko_pipeline::Pipeline;
fn main() {
    let pipeline = raiko_pipeline::sp1::Sp1Pipeline::new("provers/sp1/guest", "release");
    pipeline.bins(&["sp1-guest"], "provers/sp1/guest/elf");
    pipeline.tests(&["sp1-guest"], "provers/sp1/guest/elf");
}
