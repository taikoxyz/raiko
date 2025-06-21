pub mod boundless;
pub use boundless::Risc0BoundlessProver;

pub mod methods;

fn main() {
    let input = Vec::<u8>::new(); // GuestBatchInput as bytes
    let output = Vec::<u8>::new(); // GuestBatchOutput as bytes
    let config = serde_json::Value::default();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let prover = Risc0BoundlessProver::get().await;
        let proof: Vec<u8> = prover
            .batch_run(input, &output, &config)
            .await
            .expect("Failed to run batch proof");
        println!("Batch proof: {:?}", proof);
    });
}
