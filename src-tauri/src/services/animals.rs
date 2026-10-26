//! 宠物库扫描。
//!
//! 证据（exe 字符串）：
//!   文件：`meta.json` `enter.webm` `idle.webm` `thumbnail.png` `.revision`
//!   搜索根：`pets` `resources` `../resources` `../../resources`
//!   物种白名单：cat dog panda duck chicken fish rabbit fox bird
//!   日志：`[animals] <root> has no usable ids; the allowlist is weaker for this root`

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 物种白名单（exe 字符串表：cat dog panda duck chicken fish rabbit fox bird）。
/// 注意：实测本地宠物 `eee` 的 species 是 `goose`，且运行日志 `library=5` 说明它被正常列出，
/// 故白名单应含 goose（否则这只会被丢弃）。
pub const SPECIES_ALLOWLIST: [&str; 10] = [
    "cat", "dog", "panda", "duck", "chicken", "fish", "rabbit", "fox", "bird", "goose",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AnimalAssets {
    #[serde(default)]
    pub enter: Option<String>,
    #[serde(default)]
    pub idle: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Animal {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub species: String,
    #[serde(default)]
    pub rarity: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub bg_color: Option<String>,
    #[serde(default)]
    pub anchor_x: Option<f32>,
    #[serde(default)]
    pub scale: Option<f32>,
    #[serde(default)]
    pub loop_enter: bool,
    pub assets: AnimalAssets,
}

#[derive(Debug, Deserialize)]
struct MetaFile {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    species: String,
    #[serde(default)]
    rarity: String,
    #[serde(default)]
    personality: String,
    #[serde(default)]
    bg_color: String,
    #[serde(default)]
    anchor_x: f32,
    #[serde(default)]
    scale: f32,
}

/// 宠物资源目录候选（**按优先级排序，先扫到的先占坑**——`list()` 按 id 去重，先到先得）：
///  1. `<exe目录>\pets` —— 便携包随包素材，**最高优先级**（想换/改宠物直接换这个目录）
///  2. 数据目录（下载安装下来）`%APPDATA%\top.huyanbao.desktop\pets`
///  3. 安装目录随包资源 `<exe目录>\_up_\resources\pets`（原安装器遗留，内置 tabbycat）
///  4. exe 中出现的其余相对根：`resources`、`../resources`、`../../resources`
///
/// 证据：exe 字符串 `petsresources../resources../../resources_up_animalscats`（连排的 rodata）、
/// 以及 `<exe>\_up_\resources\pets\tabbycat` 的实际布局。
pub fn search_roots() -> Vec<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let mut roots = vec![
        // 1. 同级 pets：便携包自带，优先级最高
        exe_dir.join("pets"),
        // 2. 数据目录（petstore 下载安装 / 手动放）
        crate::state::data_dir().join("pets"),
        // 3. 安装器随包资源
        exe_dir.join("_up_").join("resources").join("pets"),
    ];
    for rel in ["resources", "../resources", "../../resources"] {
        roots.push(exe_dir.join(rel));
    }
    roots
}

fn load_animal(dir: &Path) -> Option<Animal> {
    let meta_path = dir.join("meta.json");
    let text = std::fs::read_to_string(&meta_path).ok()?;
    let meta: MetaFile = serde_json::from_str(&text).ok()?;

    if !SPECIES_ALLOWLIST.contains(&meta.species.as_str()) {
        log::warn!(
            "[animals] skipping suspicious id {} (species {})",
            meta.id,
            meta.species
        );
        return None;
    }

    let asset = |file: &str| -> Option<String> {
        let p = dir.join(file);
        p.exists().then(|| p.to_string_lossy().to_string())
    };

    Some(Animal {
        id: meta.id,
        name: meta.name,
        description: meta.description,
        species: meta.species,
        rarity: meta.rarity,
        personality: meta.personality,
        bg_color: if meta.bg_color.is_empty() { None } else { Some(meta.bg_color) },
        anchor_x: if meta.anchor_x > 0.0 { Some(meta.anchor_x) } else { None },
        scale: if meta.scale > 0.0 { Some(meta.scale) } else { None },
        loop_enter: false,
        assets: AnimalAssets {
            enter: asset("enter.webm"),
            idle: asset("idle.webm"),
            thumbnail: asset("thumbnail.png"),
        },
    })
}

/// 扫描全部根目录，按 id 去重（**先扫到的根优先**，所以 `search_roots()` 的顺序就是优先级）
pub fn list() -> Vec<Animal> {
    let mut out: Vec<Animal> = Vec::new();
    let mut usable_roots = 0usize;
    for root in search_roots() {
        if !root.is_dir() {
            continue;
        }
        let mut found_here = false;
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for entry in rd.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            if let Some(animal) = load_animal(&dir) {
                found_here = true;
                if !out.iter().any(|a| a.id == animal.id) {
                    out.push(animal);
                }
            }
        }
        if found_here {
            usable_roots += 1;
        }
    }
    if usable_roots == 0 {
        log::warn!("[animals] no root has usable ids; the allowlist is weaker for these roots");
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn get(id: &str) -> Option<Animal> {
    list().into_iter().find(|a| a.id == id)
}
