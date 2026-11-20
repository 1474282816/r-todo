pub use rtools_battery as battery;
pub mod db;
pub mod mcp;
pub mod rpc;
pub mod sync_client;
pub mod sync_manager;
pub mod tasks;
pub mod transfer;

pub const APP_NAME: &str = "rtools";
pub const PROTOCOL_VERSION: u32 = 1;
