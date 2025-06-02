/*
Uploads the guest program and creates the proof request.
Sends the proof request to Boundless.
Retrieves the proof from Boundless.
Sends the proof to the application contract.
*/


// /// Arguments of the publisher CLI.
// #[derive(Parser, Debug)]
// #[clap(author, version, about, long_about = None)]
// struct Args {
//     /// The number to publish to the EvenNumber contract.
//     #[clap(short, long)]
//     number: u32,
//     /// URL of the Ethereum RPC endpoint.
//     #[clap(short, long, env)]
//     rpc_url: Url,
//     /// Private key used to interact with the EvenNumber contract.
//     #[clap(short, long, env)]
//     wallet_private_key: PrivateKeySigner,
//     /// Submit the request offchain via the provided order stream service url.
//     #[clap(short, long, requires = "order_stream_url")]
//     offchain: bool,
//     /// Offchain order stream service URL to submit offchain requests to.
//     #[clap(long, env)]
//     order_stream_url: Option<Url>,
//     /// Storage provider to use
//     #[clap(flatten)]
//     storage_config: Option<StorageProviderConfig>,
//     /// Address of the EvenNumber contract.
//     #[clap(short, long, env)]
//     even_number_address: Address,
//     /// Address of the RiscZeroSetVerifier contract.
//     #[clap(short, long, env)]
//     set_verifier_address: Address,
//     /// Address of the BoundlessfMarket contract.
//     #[clap(short, long, env)]
//     boundless_market_address: Address,
// }