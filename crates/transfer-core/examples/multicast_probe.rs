//! 组播可用性探针：`cargo run --example multicast_probe -- 53317`
//!
//! 打印本机能否绑定/加入 LocalSend 组播组，以及失败原因。

use std::time::Duration;

use localsend::discovery::{DeviceIdentity, DiscoveryConfig, DiscoveryEvent};
use localsend::model::discovery::{DeviceType, ProtocolType, PROTOCOL_VERSION_V2};
use localsend::multicast::{MulticastDevice, DEFAULT_MULTICAST_GROUP, DEFAULT_MULTICAST_GROUP_V6};
use localsend::util::interface::InterfaceFilter;
use tokio::sync::{mpsc, oneshot};

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(53317);

    let identity = localsend::crypto::cert::generate_self_signed().expect("cert");

    let (tx, mut rx) = mpsc::channel::<DiscoveryEvent>(16);
    let (stop_tx, stop_rx) = oneshot::channel();

    let handle = localsend::discovery::start(
        DiscoveryConfig {
            group: DEFAULT_MULTICAST_GROUP,
            group_v6: Some(DEFAULT_MULTICAST_GROUP_V6),
            port,
            interface_filter: InterfaceFilter::default(),
            device: MulticastDevice {
                alias: "probe".to_string(),
                version: PROTOCOL_VERSION_V2.to_string(),
                device_model: Some("probe".to_string()),
                device_type: Some(DeviceType::Desktop),
                fingerprint: identity.fingerprint.clone(),
                port,
                protocol: ProtocolType::Https,
                download: false,
            },
            identity: DeviceIdentity {
                cert_pem: identity.certificate_pem,
                private_key_pem: identity.private_key_pem,
            },
            timeout: Duration::from_secs(1),
            event_tx: Some(tx),
        },
        stop_rx,
    )
    .await;

    match handle.multicast_error() {
        None => println!("multicast OK on port {port}"),
        Some(err) => println!("multicast FAILED on port {port}: {err:#}"),
    }

    // 顺手看看能否收到自己的广播（回环）
    let _ = handle.announce().await;
    let _ = tokio::time::timeout(Duration::from_millis(1500), rx.recv()).await;

    let _ = stop_tx.send(());
    handle.wait_stopped().await;
}
