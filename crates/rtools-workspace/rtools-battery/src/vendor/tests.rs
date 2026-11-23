//! 状态帧解析的回归用例 —— 报文全部来自真机抓包，不要手改。

use super::*;

fn a75() -> ChargeQuery {
    ChargeQuery {
        usage_page: 0xFF02,
        response_id: 0x0A,
        state_index: 7,
        charging_bit: Some(0x10),
        state_map: Default::default(),
        poll: None,
    }
}

fn vendor_spec(percent_index: usize) -> VendorSpec {
    VendorSpec {
        usage_page: 0xFF04,
        report_id: 0x06,
        percent_index,
        state_index: None,
        state_map: Default::default(),
        scale: 1.0,
        zero_means_unknown: true,
        charge_query: None,
    }
}

/// 声明之外的字节不能当电量。
/// A75 的 `[8]=0x5F(95)` 恒定不变（插拔电、放一整天都是 95），根本不是百分比；
/// 描述符里这份报文只有 8 字节，`[8]` 在声明之外。
#[test]
fn percent_beyond_declared_length_is_rejected() {
    let mut raw = vec![0u8; 64];
    raw[0] = 0x06;
    raw[8] = 0x5F;
    let (percent, note) = extract_percent(&raw, &vendor_spec(8), 8);
    assert_eq!(percent, None);
    assert!(note.is_some(), "越界时必须给界面一个说明");
}

/// 声明之内的字节正常读取（迈从 G87 就是 [3]）。
#[test]
fn percent_inside_declared_length_is_read() {
    let mut raw = vec![0u8; 64];
    raw[0] = 0x06;
    raw[3] = 42;
    let (percent, note) = extract_percent(&raw, &vendor_spec(3), 8);
    assert_eq!(percent, Some(42));
    assert!(note.is_none());
}

/// 插线时抓到的 0x05 组（100%，状态 0x11）。
#[test]
fn charging_packet_is_recognised() {
    let frame = [
        0x13, 0x0A, 0x01, 0x00, 0x04, 0x05, 0x64, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x9C,
    ];
    assert_eq!(parse_charge_event(&frame, &a75()), Some(ChargeState::Charging));
}

/// 拔线时抓到的 0x05 组（100%，状态 0x01）。
#[test]
fn discharging_packet_is_recognised() {
    let frame = [
        0x13, 0x0A, 0x01, 0x00, 0x04, 0x05, 0x64, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x8C,
    ];
    assert_eq!(
        parse_charge_event(&frame, &a75()),
        Some(ChargeState::Discharging)
    );
}

/// 0x02 组：第 6 字节 1 = 插着线。
#[test]
fn plugged_flag_packet_is_recognised() {
    let frame = [
        0x13, 0x0A, 0x01, 0x00, 0x04, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x25,
    ];
    assert_eq!(parse_charge_event(&frame, &a75()), Some(ChargeState::Charging));
}

#[test]
fn unplugged_flag_packet_is_recognised() {
    let frame = [
        0x13, 0x0A, 0x01, 0x00, 0x04, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x24,
    ];
    assert_eq!(
        parse_charge_event(&frame, &a75()),
        Some(ChargeState::Discharging)
    );
}

/// `0x11` 必须能认出来 —— 之前按取值表匹配，17 不在表里，被漏成 Unknown。
#[test]
fn high_bit_is_not_treated_as_unknown() {
    let mut frame = [0u8; 20];
    frame[0] = 0x13;
    frame[1] = 0x0A;
    frame[5] = 0x05;
    frame[7] = 0x11;
    frame[19] = frame[..19].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    assert_eq!(parse_charge_event(&frame, &a75()), Some(ChargeState::Charging));
}

/// 校验和不对的帧必须丢掉 —— 残帧、错位帧都不能变成状态。
#[test]
fn corrupted_checksum_is_rejected() {
    let mut frame = [
        0x13, 0x0A, 0x01, 0x00, 0x04, 0x05, 0x64, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x9C,
    ];
    frame[19] ^= 0xFF;
    assert_eq!(parse_charge_event(&frame, &a75()), None);
}

/// 别的报文（0x07 组、别的 Report ID）不该被当成状态。
#[test]
fn unrelated_frames_are_ignored() {
    let frame = [
        0x13, 0x07, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x1C,
    ];
    assert_eq!(parse_charge_event(&frame, &a75()), None);
}

/// 时间戳短帧（不足 8 字节）直接丢弃。
#[test]
fn short_frames_are_ignored() {
    assert_eq!(parse_charge_event(&[0x13, 0x0A], &a75()), None);
}
