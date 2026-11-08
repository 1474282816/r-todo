//! `MobileTransferConfig` → `rtodo_transfer_core::TransferConfig` 的校验。

use rtodo_transfer_core::TransferConfig;

use crate::api::transfer::MobileTransferConfig;

pub fn to_core_config(input: &MobileTransferConfig) -> Result<TransferConfig, String> {
    let alias = input.alias.trim();
    if alias.is_empty() {
        return Err("设备名称不能为空".to_string());
    }

    let receive_dir = input.receive_dir.trim();
    if receive_dir.is_empty() {
        return Err("接收目录不能为空".to_string());
    }

    if input.port == 0 {
        return Err("端口无效".to_string());
    }

    let pin = input
        .pin
        .as_deref()
        .map(str::trim)
        .filter(|pin| !pin.is_empty())
        .map(str::to_string);

    Ok(TransferConfig {
        alias: alias.to_string(),
        port: input.port,
        receive_dir: receive_dir.into(),
        identity_dir: input.identity_dir.trim().into(),
        pin,
        auto_accept_favorites: input.auto_accept_favorites,
        verify_checksums: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MobileTransferConfig {
        MobileTransferConfig {
            alias: "我的手机".into(),
            port: 53317,
            receive_dir: "/data/user/0/dev.rtodo/cache/transfer".into(),
            identity_dir: "/data/user/0/dev.rtodo/files/transfer/identity".into(),
            pin: None,
            auto_accept_favorites: false,
        }
    }

    #[test]
    fn rejects_blank_alias() {
        let mut input = sample();
        input.alias = "   ".into();
        assert_eq!(to_core_config(&input).unwrap_err(), "设备名称不能为空");
    }

    #[test]
    fn rejects_empty_receive_dir() {
        let mut input = sample();
        input.receive_dir = "  ".into();
        assert_eq!(to_core_config(&input).unwrap_err(), "接收目录不能为空");
    }

    #[test]
    fn trims_alias_and_maps_fields() {
        let mut input = sample();
        input.alias = "  我的手机  ".into();
        input.pin = Some(" 1234 ".into());

        let config = to_core_config(&input).expect("valid config");
        assert_eq!(config.alias, "我的手机");
        assert_eq!(config.port, 53317);
        assert_eq!(config.pin.as_deref(), Some("1234"));
        assert!(!config.auto_accept_favorites);
        assert_eq!(
            config.receive_dir.to_string_lossy(),
            "/data/user/0/dev.rtodo/cache/transfer"
        );
    }

    #[test]
    fn blank_pin_is_none() {
        let mut input = sample();
        input.pin = Some("  ".into());
        assert!(to_core_config(&input).expect("valid").pin.is_none());
    }

    #[test]
    fn zero_port_is_rejected() {
        let mut input = sample();
        input.port = 0;
        assert_eq!(to_core_config(&input).unwrap_err(), "端口无效");
    }
}
