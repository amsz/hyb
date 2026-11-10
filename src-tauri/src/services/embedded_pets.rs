//! 内嵌宠物素材（单文件发布包用）。
//!
//! 仓库里的 `pets/` 在编译期被打进 exe（`include_dir`），启动时解包到
//! `%APPDATA%\…\pets`（`animals::search_roots()` 的第 2 个头），于是只发一个 exe
//! 也有宠物。`<exe目录>\pets` 仍是最高优先级，想临时换宠物直接放这个目录即可。
//!
//! 注意：`../pets` 必须在编译期存在，否则 `include_dir!` 直接编译失败。

use include_dir::{include_dir, Dir};
use std::path::Path;

static PETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../pets");

fn extract(dir: &Dir<'_>, root: &Path, written: &mut usize) {
    for file in dir.files() {
        let path = root.join(file.path());
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&path, file.contents()) {
            Ok(()) => *written += 1,
            Err(e) => log::warn!("[pets] embedded {} failed: {e}", path.display()),
        }
    }
    for sub in dir.dirs() {
        extract(sub, root, written);
    }
}

/// 解包内嵌宠物；每个版本只解一次（`.embedded` 记版本号，升级后自动刷新）
pub fn install() {
    let root = crate::state::data_dir().join("pets");
    let version = env!("CARGO_PKG_VERSION");
    let stamp = root.join(".embedded");
    if std::fs::read_to_string(&stamp).map(|s| s.trim() == version).unwrap_or(false) {
        return;
    }
    let mut written = 0;
    extract(&PETS, &root, &mut written);
    if written > 0 {
        let _ = std::fs::write(&stamp, version);
        log::info!("[pets] unpacked {written} embedded files into {}", root.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自检：`pets/` 真的被编进 exe 了（包括每只宠物的素材文件）
    #[test]
    fn pets_tree_is_embedded() {
        let dir = std::env::temp_dir().join("hyb-embedded-pets-test");
        let _ = std::fs::remove_dir_all(&dir);
        let mut n = 0;
        extract(&PETS, &dir, &mut n);
        assert_eq!(PETS.dirs().count(), 5, "pets/ 下的宠物目录数变了");
        assert!(n >= 19, "内嵌文件数 {n} 少于预期");
        assert!(dir.join("01-eee/enter.webm").exists());
        assert!(dir.join("01-eee/meta.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
