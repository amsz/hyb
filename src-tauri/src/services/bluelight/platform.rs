//! `SetDeviceGammaRamp` 的 Windows 实现。
//!
//! 枚举显示设备 -> 每个设备创建 DC -> 写入 256 段 R/G/B 斜坡。
//! 所有显示器都失败时返回 Err，对应日志
//! `[bluelight] SetDeviceGammaRamp reached no display at gains …`。
//!
//! 注：windows crate 的 `CreateDCW` 有 4 个参数（driver/device/output/initdata），
//! 若你使用的 crate 版本签名不同，只需调整本文件的调用处。

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, EnumDisplayDevicesW, DISPLAY_DEVICEW, HDC,
};
// gamma 斜坡 API 在 Win32::UI::ColorSystem 下（windows 0.58 模块划分）
use windows::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};
use windows::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME;

/// 用三元组增益构造 768 个 u16（每通道 256 段，值域 0..=65535）
fn build_ramp(r: f32, g: f32, b: f32) -> [u16; 768] {
    let mut ramp = [0u16; 768];
    for i in 0..256 {
        let t = i as f32 / 255.0;
        ramp[i] = (t * r * 65535.0).round().clamp(0.0, 65535.0) as u16;
        ramp[256 + i] = (t * g * 65535.0).round().clamp(0.0, 65535.0) as u16;
        ramp[512 + i] = (t * b * 65535.0).round().clamp(0.0, 65535.0) as u16;
    }
    ramp
}

fn apply_to_device(name: &[u16], ramp: &[u16; 768]) -> bool {
    unsafe {
        // CreateDCW 直接返回 HDC（不是 Result），失败时为 invalid
        let dc: HDC = CreateDCW(PCWSTR::null(), PCWSTR(name.as_ptr()), PCWSTR::null(), None);
        if dc.is_invalid() {
            log::debug!("[bluelight] CreateDCW failed");
            return false;
        }
        let mut current = [0u16; 768];
        let mut ok = false;
        let cur_ptr = current.as_mut_ptr() as *mut core::ffi::c_void;
        if GetDeviceGammaRamp(dc, cur_ptr).as_bool() {
            if SetDeviceGammaRamp(dc, ramp.as_ptr() as *const core::ffi::c_void).as_bool() {
                ok = true;
            } else {
                // Windows 会校验斜坡；被拒绝时前端提示“系统拒绝了这档色温”
                log::debug!("[bluelight] SetDeviceGammaRamp rejected");
            }
        }
        let _ = DeleteDC(dc);
        ok
    }
}

/// 对当前会话所有活动显示设备应用色温
pub fn apply_ramp(r: f32, g: f32, b: f32) -> Result<(), String> {
    let ramp = build_ramp(r, g, b);
    let mut applied = 0usize;

    for index in 0..16u32 {
        unsafe {
            let mut dev = std::mem::zeroed::<DISPLAY_DEVICEW>();
            dev.cb = std::mem::size_of_val(&dev) as u32;
            if !EnumDisplayDevicesW(
                PCWSTR::null(),
                index,
                &mut dev,
                EDD_GET_DEVICE_INTERFACE_NAME,
            )
            .as_bool()
            {
                break;
            }
            // windows 0.58：StateFlags 是裸 u32，DISPLAY_DEVICE_ATTACHED = 0x1
            if dev.StateFlags & 0x1 == 0 {
                continue;
            }
            if apply_to_device(&dev.DeviceName, &ramp) {
                applied += 1;
            }
        }
    }

    if applied == 0 {
        Err("no display accepted the ramp".into())
    } else {
        Ok(())
    }
}

/// 恢复恒等斜坡
pub fn reset() {
    let _ = apply_ramp(1.0, 1.0, 1.0);
}
