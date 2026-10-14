//! 全局快捷键（services::shortcuts）。
//!
//! 语义与原包一致：`RegisterHotKey`（global-hotkey crate 底层即 RegisterHotKey），
//! 注册失败即“已被别的程序占用”，汇总为 `get_shortcut_conflicts` 与
//! `shortcuts:conflict` 事件。
//!
//! 日志字面量：`[shortcuts] master switch is off, nothing registered`
//!   `[shortcuts] all bindings registered`
//!   `[shortcuts] <label>: register "<combo>" failed: …`
//!   `[shortcuts] unregister_all failed: …`

use crate::state::Shared;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};

/// 配置字段 -> 中文标签（与设置页 ROWS 一致）
pub const SHORTCUT_LABELS: [(&str, &str); 7] = [
    ("shortcutEyeToggle", "开关护眼"),
    ("shortcutPause", "暂停 / 恢复计时"),
    ("shortcutRest", "立即休息"),
    ("shortcutShow", "打开主界面"),
    ("shortcutQuit", "退出"),
    ("shortcutFilterUp", "增强滤镜 5%"),
    ("shortcutFilterDown", "减弱滤镜 5%"),
];

/// keyboard-types 的 `Code` 枚举（global_hotkey::hotkey::Code 的重导出）：
/// 字母 `KeyA..KeyZ`、数字 `Digit0..Digit9`、方向键 `ArrowUp..`、`Equal`/`Minus`/`F1..F12`
fn letter_code(c: char) -> Option<Code> {
    use Code::*;
    Some(match c.to_ascii_uppercase() {
        'A' => KeyA, 'B' => KeyB, 'C' => KeyC, 'D' => KeyD, 'E' => KeyE, 'F' => KeyF,
        'G' => KeyG, 'H' => KeyH, 'I' => KeyI, 'J' => KeyJ, 'K' => KeyK, 'L' => KeyL,
        'M' => KeyM, 'N' => KeyN, 'O' => KeyO, 'P' => KeyP, 'Q' => KeyQ, 'R' => KeyR,
        'S' => KeyS, 'T' => KeyT, 'U' => KeyU, 'V' => KeyV, 'W' => KeyW, 'X' => KeyX,
        'Y' => KeyY, 'Z' => KeyZ,
        '0' => Digit0, '1' => Digit1, '2' => Digit2, '3' => Digit3, '4' => Digit4,
        '5' => Digit5, '6' => Digit6, '7' => Digit7, '8' => Digit8, '9' => Digit9,
        _ => return None,
    })
}

fn parse_combo(combo: &str) -> Option<HotKey> {
    if combo.trim().is_empty() {
        return None;
    }
    let mut mods = Modifiers::empty();
    let mut key: Option<Code> = None;
    for part in combo.split('+').map(|p| p.trim()) {
        match part.to_ascii_lowercase().as_str() {
            "commandorcontrol" | "ctrl" | "control" | "command" | "meta" => {
                mods |= Modifiers::CONTROL
            }
            "alt" => mods |= Modifiers::ALT,
            "shift" => mods |= Modifiers::SHIFT,
            other => {
                let code = match other {
                    "up" => Code::ArrowUp,
                    "down" => Code::ArrowDown,
                    "left" => Code::ArrowLeft,
                    "right" => Code::ArrowRight,
                    "equal" => Code::Equal,
                    "minus" => Code::Minus,
                    single if single.len() == 1 => {
                        letter_code(single.chars().next()?)?
                    }
                    f if f.starts_with('f') && f[1..].parse::<u8>().is_ok() => {
                        let n: u8 = f[1..].parse().ok()?;
                        match n {
                            1 => Code::F1,
                            2 => Code::F2,
                            3 => Code::F3,
                            4 => Code::F4,
                            5 => Code::F5,
                            6 => Code::F6,
                            7 => Code::F7,
                            8 => Code::F8,
                            9 => Code::F9,
                            10 => Code::F10,
                            11 => Code::F11,
                            12 => Code::F12,
                            _ => return None,
                        }
                    }
                    _ => return None,
                };
                key = Some(code);
            }
        }
    }
    Some(HotKey::new(Some(mods), key?))
}

struct Registered {
    hotkey: HotKey,
    field: String,
}

/// `GlobalHotKeyManager` 内部持有 Windows 句柄（`*mut c_void`），windows-rs 未为其
/// 实现 Send/Sync。它只会通过 `Mutex<ShortcutEngine>` 串行访问（注册/反注册/查询
/// 都在锁内），所以在此补齐线程安全性 —— 与 tauri-plugin-global-shortcut 同样的处理。
struct HotKeyManager(GlobalHotKeyManager);
unsafe impl Send for HotKeyManager {}
unsafe impl Sync for HotKeyManager {}

/// 已注册的热键集合（进程内单例）
pub struct ShortcutEngine {
    registered: Vec<Registered>,
    manager: HotKeyManager,
}

impl ShortcutEngine {
    pub fn new() -> Self {
        Self {
            registered: Vec::new(),
            manager: HotKeyManager(
                GlobalHotKeyManager::new()
                    .map_err(|e| format!("hotkey manager: {e}"))
                    .expect("failed to create global hotkey manager"),
            ),
        }
    }

    /// 按当前配置重新注册；返回未注册成功的中文标签（含组合）
    pub fn reregister(&mut self, shared: &Shared) -> Vec<String> {
        self.unregister_all();

        let (enabled, bindings): (bool, Vec<(String, String, String)>) = {
            let guard = crate::state::lock(shared);
            (
                guard.config.shortcuts_enabled,
                SHORTCUT_LABELS
                    .iter()
                    .map(|(field, label)| {
                        (
                            field.to_string(),
                            label.to_string(),
                            guard.config.config_value(field).to_string(),
                        )
                    })
                    .collect(),
            )
        };

        if !enabled {
            log::info!("[shortcuts] master switch is off, nothing registered");
            return Vec::new();
        }

        let mut conflicts = Vec::new();
        for (field, label, combo) in bindings {
            let Some(hotkey) = parse_combo(&combo) else { continue };
            match self.manager.0.register(hotkey.clone()) {
                Ok(()) => self.registered.push(Registered { hotkey, field }),
                Err(e) => {
                    log::warn!("[shortcuts] {label}: register \"{combo}\" failed: {e}");
                    conflicts.push(format!("{label}（{combo}）"));
                }
            }
        }
        if conflicts.is_empty() {
            log::info!("[shortcuts] all bindings registered");
        }
        conflicts
    }

    fn unregister_all(&mut self) {
        for item in self.registered.drain(..) {
            if let Err(e) = self.manager.0.unregister(item.hotkey) {
                log::warn!("[shortcuts] unregister_all failed: {e}");
            }
        }
    }

    /// 消费 GlobalHotKeyEvent，返回命中的配置字段
    pub fn dispatch(&self, id: u32) -> Option<String> {
        self.registered
            .iter()
            .find(|r| r.hotkey.id() == id)
            .map(|r| r.field.clone())
    }
}

/// 注册监听线程：把热键事件翻译成应用动作
pub fn spawn_listener(
    app: tauri::AppHandle,
    shared: Shared,
    engine: std::sync::Arc<std::sync::Mutex<ShortcutEngine>>,
) {
    let receiver = GlobalHotKeyEvent::receiver();
    std::thread::spawn(move || {
        while let Ok(event) = receiver.recv() {
            if event.state() != HotKeyState::Pressed {
                continue;
            }
            let field = { engine.lock().unwrap_or_else(|e| e.into_inner()).dispatch(event.id) };
            let Some(field) = field else { continue };
            crate::commands::on_shortcut(&app, &shared, &field);
        }
    });
}
