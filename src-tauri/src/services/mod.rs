//! 服务层：按原包模块拆分（exe 中的模块路径字符串）。
//!
//!   services::bluelight      色温 / gamma
//!   services::timer          工作-休息计时与休息触发
//!   services::windows        四个窗口 + 托盘 + 显示器变化
//!   services::shortcuts      全局热键（RegisterHotKey 语义）
//!   services::activity       击键/点击计数（全局输入钩子，仅计数）
//!   services::animals        宠物库扫描与白名单
//!   services::petstore       远端宠物仓库（主源 + 镜像 + sha256 校验）
//!   services::updater        检查更新 / 下载 / 计划任务提权安装
//!   services::reporter       遥测上报（日报/DAU/新装/截图）

pub mod activity;
pub mod animals;
pub mod bluelight;
pub mod petstore;
pub mod reporter;
pub mod shortcuts;
pub mod timer;
pub mod updater;
pub mod windows;
