pub mod block_builder;
pub mod execute;
#[cfg(not(target_os = "zkvm"))]
pub mod host;
pub mod precheck;
pub mod prepare;
pub mod protocol_instance;
pub mod utils;
#[cfg(not(target_os = "zkvm"))]
pub mod verify;
pub mod blob_utils;

pub enum Layer {
    L1,
    L2,
}