//! 设备发现：UDP 组播 + 手动 IP / 子网扫描。

use std::net::{Ipv4Addr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use localsend::discovery::{
    DeviceChannel as LsDeviceChannel, DeviceIdentity, DiscoveredDevice, DiscoveryConfig,
    DiscoveryEvent, DiscoveryHandle, HttpChannel as LsHttpChannel, StatefulDevice,
};
use localsend::http::dto_v2::RegisterDtoV2;
use localsend::model::discovery::{DeviceType, ProtocolType, PROTOCOL_VERSION_V2};
use localsend::multicast::{MulticastDevice, DEFAULT_MULTICAST_GROUP, DEFAULT_MULTICAST_GROUP_V6};
use localsend::util::interface::InterfaceFilter;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::identity::Identity;
use crate::{DeviceChannel, DeviceInfo, Protocol, TransferConfig};

/// 主动发现时等待对端回复的宽限时间。
const DISCOVER_GRACE: Duration = Duration::from_millis(1200);

pub(crate) struct DiscoveryRuntime {
    pub(crate) handle: Arc<DiscoveryHandle>,
    stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl DiscoveryRuntime {
    pub(crate) async fn stop(&self) {
        if let Some(tx) = self.stop_tx.lock().await.take() {
            let _ = tx.send(());
        }
        self.handle.wait_stopped().await;
    }

    pub(crate) fn devices(&self) -> Vec<DeviceInfo> {
        self.handle
            .devices()
            .iter()
            .map(device_info_from_stateful)
            .collect()
    }
}

pub(crate) async fn start(
    config: &TransferConfig,
    identity: &Identity,
    event_tx: mpsc::Sender<DiscoveryEvent>,
) -> DiscoveryRuntime {
    let (stop_tx, stop_rx) = oneshot::channel();

    let handle = localsend::discovery::start(
        DiscoveryConfig {
            group: DEFAULT_MULTICAST_GROUP,
            group_v6: Some(DEFAULT_MULTICAST_GROUP_V6),
            port: config.port,
            interface_filter: InterfaceFilter::default(),
            device: MulticastDevice {
                alias: config.alias.clone(),
                version: PROTOCOL_VERSION_V2.to_string(),
                device_model: Some(platform_name()),
                device_type: Some(DeviceType::Desktop),
                fingerprint: identity.fingerprint.clone(),
                port: config.port,
                protocol: ProtocolType::Https,
                download: false,
            },
            identity: DeviceIdentity {
                cert_pem: identity.cert_pem.clone(),
                private_key_pem: identity.private_key_pem.clone(),
            },
            timeout: localsend::discovery::DEFAULT_DISCOVERY_TIMEOUT,
            event_tx: Some(event_tx),
        },
        stop_rx,
    )
    .await;

    DiscoveryRuntime {
        handle: Arc::new(handle),
        stop_tx: Mutex::new(Some(stop_tx)),
    }
}

/// 主动发现：先广播自己（让对端回调 register），再用已知网卡做 /24 兜底扫描。
pub(crate) async fn discover_active(discovery: &DiscoveryRuntime, port: u16, timeout: Duration) {
    let grace = if timeout.is_zero() { DISCOVER_GRACE } else { timeout };
    let interface_ips = local_ipv4_addresses();
    let _ = discovery
        .handle
        .discover_staged(
            Vec::new(),
            interface_ips,
            port,
            ProtocolType::Https,
            grace,
        )
        .await;
}

/// 手动 IP 兜底：直接向该地址发 register。
pub(crate) async fn discover_manual(
    discovery: &DiscoveryRuntime,
    host: &str,
    port: u16,
) -> Option<StatefulDevice> {
    discovery
        .handle
        .discover(host, port, ProtocolType::Https)
        .await
        .ok()
        .flatten()
}

/// 把服务端收到的 register 事件喂回发现存储，使其出现在设备列表里。
pub(crate) async fn remember_registered(
    discovery: &DiscoveryRuntime,
    ip: &str,
    info: &RegisterDtoV2,
    cert_fingerprint: Option<&str>,
) {
    let fingerprint = cert_fingerprint
        .map(|s| s.to_ascii_uppercase())
        .unwrap_or_else(|| info.fingerprint.to_ascii_uppercase());

    discovery
        .handle
        .add_device(DiscoveredDevice {
            alias: info.alias.clone(),
            version: info.version.clone(),
            device_model: info.device_model.clone(),
            device_type: info.device_type.clone(),
            fingerprint,
            channel: LsDeviceChannel::Http(LsHttpChannel {
                host: ip.to_string(),
                port: info.port,
                protocol: info.protocol,
            }),
            download: info.download,
        })
        .await;
}

pub(crate) fn device_info_from_stateful(device: &StatefulDevice) -> DeviceInfo {
    DeviceInfo {
        alias: device.device.alias.clone(),
        version: device.device.version.clone(),
        device_model: device.device.device_model.clone(),
        device_type: device.device.device_type.as_ref().map(device_type_str),
        fingerprint: device.device.fingerprint.clone(),
        download: device.device.download,
        channels: device
            .get_ranked_channels()
            .into_iter()
            .filter_map(|channel| channel.http())
            .map(|http| DeviceChannel {
                host: http.host.clone(),
                port: http.port,
                protocol: protocol_of(http.protocol),
            })
            .collect(),
    }
}

fn device_type_str(t: &DeviceType) -> String {
    match t {
        DeviceType::Mobile => "mobile",
        DeviceType::Desktop => "desktop",
        DeviceType::Web => "web",
        DeviceType::Headless => "headless",
        DeviceType::Server => "server",
    }
    .to_string()
}

pub(crate) fn protocol_of(p: ProtocolType) -> Protocol {
    match p {
        ProtocolType::Http => Protocol::Http,
        ProtocolType::Https => Protocol::Https,
    }
}

pub(crate) fn protocol_type_of(p: Protocol) -> ProtocolType {
    match p {
        Protocol::Http => ProtocolType::Http,
        Protocol::Https => ProtocolType::Https,
    }
}

pub(crate) fn platform_name() -> String {
    match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        "android" => "Android",
        "ios" => "iOS",
        other => other,
    }
    .to_string()
}

/// 本机可用的 IPv4 地址（用于 /24 兜底扫描）。
///
/// 用「向公网地址 connect 一个 UDP socket 再读 local_addr」的经典技巧，
/// 不实际发包，也不依赖额外 crate。
pub(crate) fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = Vec::new();
    for probe in ["8.8.8.8:80", "1.1.1.1:80", "192.168.1.1:80"] {
        let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
            continue;
        };
        if socket.connect(probe).is_err() {
            continue;
        }
        if let Ok(addr) = socket.local_addr() {
            if let std::net::IpAddr::V4(v4) = addr.ip() {
                if !v4.is_loopback() && !v4.is_unspecified() && !out.contains(&v4) {
                    out.push(v4);
                }
            }
        }
    }
    out
}
