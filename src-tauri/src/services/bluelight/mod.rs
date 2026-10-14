//! 色温（蓝光过滤）服务。
//!
//! 主通道：`SetDeviceGammaRamp` 按显示器写入 R/G/B 增益（平台实现见 `platform`）。
//! 兜底通道：独立透明蒙层窗口 `filter-overlay`（alpha 上限 0.68，见前端 FilterOverlay）。
//!
//! 日志字面量来自 exe：
//!   `[bluelight] SetDeviceGammaRamp reached no display at gains r=… g=… b=…`
//!   `Windows validates the ramp and rejects ones too …`

pub mod platform;

use crate::services::windows;
use crate::state::{EyeState, Shared};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

pub const OVERLAY_ALPHA_CAP: f32 = 0.68;

/// 色温预设：`list_color_presets` 返回值
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorPreset {
    pub id: &'static str,
    pub name: &'static str,
    /// `"r, g, b"`（与 `EyeState.color` 同格式）
    pub color: &'static str,
    pub description: &'static str,
}

/// 六个预设：描述文案逐字来自 exe 字符串表；
/// id（pale_yellow / warm_yellow / eye_green / orange_warm / mint）在代码段中出现，
/// 第五个 id（strong_warm）为推断。
pub const PRESETS: [ColorPreset; 6] = [
    ColorPreset {
        id: "pale_yellow",
        name: "淡黄",
        color: "255, 214, 120",
        description: "轻度暖色，适合白天办公",
    },
    ColorPreset {
        id: "warm_yellow",
        name: "暖黄",
        color: "255, 168, 36",
        description: "经典护眼暖黄，平衡舒适",
    },
    ColorPreset {
        id: "eye_green",
        name: "护眼绿",
        color: "110, 186, 108",
        description: "薄荷绿调，清爽护眼",
    },
    ColorPreset {
        id: "orange_warm",
        name: "橙光",
        color: "255, 140, 24",
        description: "偏橙暖色，傍晚更舒适",
    },
    ColorPreset {
        id: "strong_warm",
        name: "强暖",
        color: "255, 110, 20",
        description: "强力暖色，减少蓝光刺激",
    },
    ColorPreset {
        id: "mint",
        name: "青薄荷",
        color: "56, 168, 152",
        description: "青薄荷色，清新舒缓",
    },
];

/// 夜间助眠用的强力暖色（与 nightMode 配套）
pub const NIGHT_COLOR: &str = "255, 110, 20";

pub fn preset_by_id(id: &str) -> &'static ColorPreset {
    PRESETS.iter().find(|p| p.id == id).unwrap_or(&PRESETS[1])
}

fn parse_rgb(s: &str) -> (f32, f32, f32) {
    let mut it = s.split(',').map(|p| p.trim().parse::<f32>().unwrap_or(255.0));
    (it.next().unwrap_or(255.0), it.next().unwrap_or(255.0), it.next().unwrap_or(255.0))
}

/// 按强度把预设颜色换算成 gamma 增益：
/// `gain = 1 - (1 - channel/255) * strength/100`
pub fn gains_for(strength: u32, color: &str) -> (f32, f32, f32) {
    let (r, g, b) = parse_rgb(color);
    let k = strength.min(100) as f32 / 100.0;
    let mut gr = 1.0 - (1.0 - r / 255.0) * k;
    let mut gg = 1.0 - (1.0 - g / 255.0) * k;
    let mut gb = 1.0 - (1.0 - b / 255.0) * k;
    // Windows 会拒绝“全 1.0”的恒等斜坡（见日志），略微下压避免无效写入
    if gr >= 1.0 && gg >= 1.0 && gb >= 1.0 {
        gr = 0.999;
        gg = 0.999;
        gb = 0.999;
    }
    (gr.clamp(0.0, 1.0), gg.clamp(0.0, 1.0), gb.clamp(0.0, 1.0))
}

/// 当前生效的颜色（夜间模式覆盖预设色）
pub fn active_color(cfg: &crate::state::AppConfig) -> String {
    if cfg.night_mode {
        NIGHT_COLOR.to_string()
    } else {
        preset_by_id(&cfg.color_preset).color.to_string()
    }
}

/// 重新计算并应用护眼状态；成功后广播 `eye:changed`
pub fn refresh(app: &tauri::AppHandle, shared: &Shared) -> EyeState {
    let (enabled, strength, overlay, night, preset) = {
        let guard = crate::state::lock(shared);
        (
            guard.config.eye_enabled,
            guard.config.filter_strength,
            guard.config.overlay_strength,
            guard.config.night_mode,
            guard.config.color_preset.clone(),
        )
    };
    let _ = preset;

    let color = {
        let guard = crate::state::lock(shared);
        active_color(&guard.config)
    };

    let gamma_working = if enabled {
        let (gr, gg, gb) = gains_for(strength, &color);
        match platform::apply_ramp(gr, gg, gb) {
            Ok(()) => true,
            Err(e) => {
                log::warn!(
                    "[bluelight] SetDeviceGammaRamp reached no display at gains \
                     r={gr:.3} g={gg:.3} b={gb:.3}: {e}"
                );
                false
            }
        }
    } else {
        // 关闭滤镜：恢复恒等斜坡
        let _ = platform::apply_ramp(0.999, 0.999, 0.999);
        true
    };

    let alpha = if enabled { overlay as f32 / 100.0 * OVERLAY_ALPHA_CAP } else { 0.0 };

    let eye = EyeState {
        enabled,
        strength,
        alpha,
        color,
        night_mode: night,
        preset_id: preset,
        gamma_working,
    };

    {
        let mut guard = crate::state::lock(shared);
        guard.eye = eye.clone();
    }
    let _ = app.emit("eye:changed", &eye);
    // 蒙层 alpha=0 时隐藏 overlay 窗口（原包靠 webview 透明，这里直接隐藏更彻底）
    windows::set_filter_overlay_visible(app, alpha > 0.0);
    eye
}

/// 恢复系统默认色温（退出/休眠时调用）
pub fn reset() {
    let _ = platform::apply_ramp(1.0, 1.0, 1.0);
}
