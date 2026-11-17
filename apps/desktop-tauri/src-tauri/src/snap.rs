//! 窗口贴边吸附 + 收起为边条
//!
//! 交互模型：
//! 1. 用户拖动窗口 -> 松手后，若离屏幕边/角在 8px（逻辑像素）内 -> snap 到边
//! 2. 若"收起为边条"开关开着 -> snap 后再滑到屏幕外，只露 3px
//! 3. 用户鼠标进入这 3px 露出的区域 -> 展开回 snap 位置
//! 4. 鼠标离开窗口 -> 再次收起
//!
//! 阈值和收起策略前端配置，Rust 只暴露纯计算 + 位置设置命令。

use serde::Serialize;

const SNAP_THRESHOLD_LOGICAL_PX: f64 = 8.0;
const COLLAPSE_PEEK_LOGICAL_PX: f64 = 10.0;

#[derive(Debug, Clone, Serialize)]
pub struct SnapTarget {
    pub edge: String,
    pub x: i32,
    pub y: i32,
}

/// 拖动结束后前端调用：返回该 snap 到哪、目标坐标。null 表示不该吸附。
#[tauri::command]
pub async fn get_snap_target(
    window: tauri::Window,
) -> Result<Option<SnapTarget>, String> {
    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no current monitor".to_string())?;
    let scale = monitor.scale_factor();
    let threshold = (SNAP_THRESHOLD_LOGICAL_PX * scale) as i32;

    let mon_pos = monitor.position();
    let mon_size = monitor.size();
    let win_pos = window.outer_position().map_err(|e| e.to_string())?;
    let win_size = window.outer_size().map_err(|e| e.to_string())?;

    let left = win_pos.x - mon_pos.x;
    let top = win_pos.y - mon_pos.y;
    let right = (mon_pos.x + mon_size.width as i32) - (win_pos.x + win_size.width as i32);
    let bottom = (mon_pos.y + mon_size.height as i32) - (win_pos.y + win_size.height as i32);

    let s_left = left.abs() <= threshold;
    let s_right = right.abs() <= threshold;
    let s_top = top.abs() <= threshold;
    let s_bottom = bottom.abs() <= threshold;

    if !s_left && !s_right && !s_top && !s_bottom {
        return Ok(None);
    }

    let target_x = if s_left {
        mon_pos.x
    } else if s_right {
        mon_pos.x + mon_size.width as i32 - win_size.width as i32
    } else {
        win_pos.x
    };
    let target_y = if s_top {
        mon_pos.y
    } else if s_bottom {
        mon_pos.y + mon_size.height as i32 - win_size.height as i32
    } else {
        win_pos.y
    };

    let edge = match (s_left, s_right, s_top, s_bottom) {
        (true, _, true, _) => "top-left",
        (_, true, true, _) => "top-right",
        (true, _, _, true) => "bottom-left",
        (_, true, _, true) => "bottom-right",
        (true, _, _, _) => "left",
        (_, true, _, _) => "right",
        (_, _, true, _) => "top",
        (_, _, _, true) => "bottom",
        _ => return Ok(None),
    };

    Ok(Some(SnapTarget {
        edge: edge.into(),
        x: target_x,
        y: target_y,
    }))
}

/// 计算"收起"目标位置：贴边方向上只露 COLLAPSE_PEEK_LOGICAL_PX 像素。
/// 返回目标 (x, y)，由前端负责缓动移动。
#[tauri::command]
pub async fn get_collapse_target(
    window: tauri::Window,
    edge: String,
) -> Result<(i32, i32), String> {
    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no current monitor".to_string())?;
    let scale = monitor.scale_factor();
    let peek = (COLLAPSE_PEEK_LOGICAL_PX * scale) as i32;

    let mon_pos = monitor.position();
    let mon_size = monitor.size();
    let win_pos = window.outer_position().map_err(|e| e.to_string())?;
    let win_size = window.outer_size().map_err(|e| e.to_string())?;

    let (x, y) = match edge.as_str() {
        "left" | "top-left" | "bottom-left" => {
            (mon_pos.x - win_size.width as i32 + peek, win_pos.y)
        }
        "right" | "top-right" | "bottom-right" => {
            (mon_pos.x + mon_size.width as i32 - peek, win_pos.y)
        }
        "top" => (win_pos.x, mon_pos.y - win_size.height as i32 + peek),
        "bottom" => (win_pos.x, mon_pos.y + mon_size.height as i32 - peek),
        _ => return Err(format!("unknown edge: {edge}")),
    };
    Ok((x, y))
}
