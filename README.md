# 护眼宝（huyanbao）重建源码

依据已安装的 `huyanbao.exe` **0.1.4**（PE32+ x86-64，SHA-256 `ebd3ac799372165f8653e0596657e5fcff055ff13abd997d2ab7b92dbfa17ada`）
逆向重建的工程源码。详细分析见 [../REPORT.md](../REPORT.md)。

> **这不是原厂源码**：结构、命令名、事件名、配置字段、UI 文案、日志文案均来自二进制证据；
> 具体算法与少量未验证细节以 `[INFERRED]` 标注。运行时行为请以原程序为准。

## 目录

```
huyanbao-rebuild/
├─ index.html                 Vite 入口（原包 index.html 还原）
├─ package.json / vite.config.js
├─ src/                       Vue 3 + naive-ui 前端
│  ├─ main.js  App.vue  router.js
│  ├─ store/app.js            全局状态（对应原包 Gl store）
│  ├─ lib/ipc.js              Tauri invoke 封装（命令名与原包一致）
│  ├─ components/             MainLayout / AppIcon / icons.js（图标从原包还原）
│  ├─ views/                  EyePage HealthPage PetPage SettingsPage AboutPage
│  ├─ overlays/               FilterOverlay AnimalOverlay EarOverlay
│  ├─ assets/logo.png         标题栏/侧栏/关于页品牌图（与应用图标同图）
│  └─ styles/app.css          原包 /assets/index-BOpHBnO2.css 还原（去掉 scoped 哈希）
└─ src-tauri/                 Rust 后端（crate 名 huyanbao_lib，与原包符号一致）
   ├─ tauri.conf.json         identifier=top.huyanbao.desktop
   ├─ capabilities/default.json
   ├─ icons/icon.ico          应用图标（自定义；原 exe 还原版备份为 icon.original.ico）
   └─ src/
      ├─ lib.rs               Builder + setup + 窗口事件
      ├─ commands.rs          全部 IPC 命令
      ├─ state.rs             AppConfig(44+2 字段) / DailyStats / EyeState / TimerState
      ├─ error.rs
      └─ services/
         ├─ bluelight/        SetDeviceGammaRamp + 蒙层 + 6 个色温预设
         ├─ timer.rs          工作-休息计时、长休、离开暂停、预提醒
         ├─ windows.rs        4 个窗口 + 托盘 + 全屏/系统忙检测
         ├─ shortcuts.rs      全局热键 + 冲突上报
         ├─ activity.rs       击键/点击计数（WH_*_LL，仅计数）
         ├─ animals.rs        宠物扫描 + 物种白名单
         ├─ petstore.rs       主源/镜像 + sha256 + _up_ 暂存
         ├─ updater.rs        检查更新 + schtasks 提权安装
         └─ reporter.rs       遥测（建议默认关闭，见下）
```

## 构建

需要 Node 20+ 与 Rust（`rustup` + MSVC 工具链）：

```bash
npm install
npm run build          # 前端 → dist/（release 构建会把它内嵌进 exe）

check.bat              # = cargo check                         ✅ 0 error / 0 warning
cargo-env.bat build    # = cargo build（debug，连 Vite dev server）

# 打包（推荐）：出可直接运行的 exe + 便携 zip
powershell -ExecutionPolicy Bypass -File package.ps1
powershell -ExecutionPolicy Bypass -File package.ps1 -Production   # 正式版配置
powershell -ExecutionPolicy Bypass -File package.ps1 -Installer    # 额外出 NSIS 安装包
```

> ⚠ **`cargo build --release` 必须加 `--features custom-protocol`**，否则 exe 启动后报
> `ERR_CONNECTION_REFUSED`。原因：`tauri-macros` 里 `dev = cfg!(not(feature = "custom-protocol"))`，
> 没开这个 feature 就认为是开发模式 → 前端资产表为空 → 退回连 `devUrl`(localhost:1420)。
> `tauri build` 会自动带上，裸 cargo 必须显式写（`package.ps1` 已带）。

**验证状态**

| 部分 | 状态 |
|---|---|
| 前端 `vite build` | ✅ 2705 modules，5 页面 + 3 overlay 全部编译 |
| 依赖解析 `Cargo.lock` | ✅ 593 crates |
| 后端 `cargo check` | ✅ 0 error / 0 warning |
| 后端 `cargo build`（debug） | ✅ `target/debug/huyanbao.exe` |
| **打包 `package.ps1`** | ✅ `release/huyanbao-0.1.4-win-x64-portable.zip`（5.9MB，含 SHA256） |
| **release 独立运行** | ✅ 停掉 Vite 后双击即可：`/assets/` 22 条已内嵌，UI 完整渲染、计时正常 |
| 链接产物自检 | ✅ 导入表含 `gdi32!SetDeviceGammaRamp`、`user32!SetWindowsHookExW`、`user32!RegisterHotKey`、`user32!GetLastInputInfo`、`shell32!Shell_NotifyIconW`、`shell32!SHQueryUserNotificationState` |

### 构建环境（本机踩过的坑）

1. **链接器被 coreutils 抢名**：机器级 PATH 里有 `C:\Program Files\coreutils\bin`
   （uutils 0.8.0），Git Bash 又把 `/usr/bin`（GNU coreutils 8.32）插到最前，两者的
   `link.exe` 都会遮蔽 MSVC 的。**不建议从 PATH 摘除 coreutils**（shell 依赖 ls/grep/sed，
   且两个来源都要动）—— 而是用 `cargo-env.bat` 把 VC 目录**前置**到 PATH。
2. **VS18 工具集版本错位**：`Microsoft.VCToolsVersion.default.txt` 指向 `14.51.36231`，
   但那个目录只有一个 `Auxiliary`（未装全），`vcvarsall.bat x64` 会“初始化成功”却**不注入
   MSVC 路径**。`cargo-env.bat` 手工构造等价环境，改用完整的 `14.44.35207`。
3. **批处理里别写中文**：cmd 按 GBK 解析会把注释切碎，`cargo-env.bat` 保持纯 ASCII + CRLF。
4. crates.io CDN 在本机只有 ~40KB/s 且会假死，`Cargo.lock` 里的 593 个 crate 最后是按
   `missing.txt` 用 `curl -C -` 断点续传补齐的（`dl.py`/`dl.log`），之后 `CARGO_NET_OFFLINE=true`。

### 运行方式（已踩坑并验证）

**debug 构建走 `devUrl`（`http://localhost:1420`），不走内嵌 dist —— 必须先起 Vite**，
否则主窗口是一整屏 `ERR_CONNECTION_REFUSED`（见 `screen.png`）：

```bash
npm run dev               # 终端 A：Vite 监听 1420
cmd //c src-tauri\cargo-env.bat run   # 终端 B：或直接双击 target\debug\huyanbao.exe
# 关：taskkill /im huyanbao.exe /f
```
`npm run tauri dev` 会把两者一起拉起，等价于上面两步。
要脱离 Vite 跑，用 `cargo-env.bat build --release`（release 构建才嵌入 `dist/`）。

### dev 隔离（已改，发布时要改回）

| 项 | dev 值 | 发布值 | 位置 |
|---|---|---|---|
| identifier | `top.huyanbao.dev` | `top.huyanbao.desktop` | `tauri.conf.json` |
| 数据目录 | `%APPDATA%\top.huyanbao.dev` | `top.huyanbao.desktop` | `state.rs::APP_DIR_NAME` |
| 日志目录 | `%LOCALAPPDATA%\top.huyanbao.dev\logs` | 同上 | `state.rs::data_dir/log_dir` |
| 自启计划任务 | `HuyanBao Dev Auto Start` | `HuyanBao Auto Start` | `state.rs::AUTOSTART_TASK` |
| 更新计划任务 | `Update HuyanBao Dev - Skip UAC` | `Update HuyanBao - Skip UAC` | `updater.rs::TASK_NAME` |
| 遥测 / 自更新 | 关闭（启动日志可见） | 开启 | `state.rs::IS_DEV` + `lib.rs` |

**三处（identifier / APP_DIR_NAME / IS_DEV）必须同步改**，否则会与已安装的护眼宝抢
`config.json`、热键和计划任务。

宠物：dev 数据目录是空的，启动会打 `[animals] no root has usable ids`。已把 5 只复制到
`%APPDATA%\top.huyanbao.dev\pets\`，并把 tabbycat 放进 `target\debug\_up_\resources\pets\`
（对应 `search_roots` 的两条根），重启后不再告警。

### 运行验证结果（2026-10-13）

| 检查项 | 结果 |
|---|---|
| 进程 | ✅ `target\debug\huyanbao.exe` 存活，~67MB，另有 6 个 `msedgewebview2` 子进程 |
| UI 渲染 | ✅ `screen2.png`/`screen3.png`：主窗口 + 侧栏 + 55% 环形 + 三卡片 + `休息倒计时 49:26`（计时线程在走） |
| WebView 隔离 | ✅ `%LOCALAPPDATA%\top.huyanbao.dev\EBWebView\` 独立 profile |
| 数据隔离 | ✅ 新目录只生成 `history.json`/`install_id.txt`；原 `top.huyanbao.desktop\config.json` mtime 前后一致 |
| 遥测/更新 | ✅ 日志 `[reporter] dev build: telemetry disabled`、`[updater] dev build: startup check disabled` |
| 启动日志 | ✅ `[startup] version=0.1.4 … dataDir=…top.huyanbao.dev … installId=5331…` |
| 宠物同步 | ⚠ 主源 `pets.huyanbao.workers.dev` 不可达；`address.txt` 返回镜像 `t2m0g2c4d.oss-ap-southeast-1.aliyuncs.com` 也连不上 → `no mirror … could be used`（纯环境网络问题） |

### 差异修复记录（已验证）

| # | 问题 | 根因 | 修复 |
|---|---|---|---|
| 1 | 与 huyanbao.cc 通讯 | petstore 同步 / 更新检查 / 遥测 / tongji.js 注入 | 新增 `state::ALLOW_HUYANBAO_NET=false`，统一在 `lib.rs`/`commands.rs` 收口；日志可见 `[pets] offline policy: pet store sync disabled` |
| 2 | 侧栏偏宽、关闭按钮偏大 | 原包 CSS 里 `svg{width:44px;height:44px}` 本是**小云豆组件的 scoped 规则**，我整表剥掉 `[data-v-…]` 后变成了全局规则，把所有 svg 撼到 44px | 把 182 条 scoped 规则按 `data-v-xxxx` 归位到 9 个 SFC 的 `<style scoped>`，`app.css` 只留 16 条真全局规则 |
| 3 | 窗口尺寸与原版不符 | 我猜的 980×680；原版截图量得 698×791 | `tauri.conf.json` 改为 700×790（min 640×560） |
| 4 | 关闭后遮罩常驻、拖动残影 | ① `ensure_filter_overlay` 在 `bluelight::refresh` 之后执行，alpha=0 的隐藏被后续创建覆盖；② 全屏置顶窗常驻 | `refresh` 改到建窗之后；新增 `set_filter_overlay_visible`，alpha=0 时直接 `hide`；`set_background_color(0,0,0,0)` 确保窗口/webview 层透明 |
| 5 | 休息结束后宠物窗口不消失 | `end_rest` 主动关窗触发 `Destroyed` 事件，把 `OVERLAY_CLOSED` 置位 → **下一次休息刚建窗就被误判为“用户关闭”** | 新增 `SUPPRESS_OVERLAY_CLOSE_EVENT`，程序主动关闭不置位；实测日志变成 `[rest] ended: … / next work target 60s` |
| 6 | 托盘菜单顺序 | 我拼的顺序不对 | 改为原包 rodata 字面量顺序：暂停/立即休息/打开主界面/退出/─/增强/减弱/开关护眼 |
| 7 | 侧栏 4 个图标“丢失”（只剩宠物爪印） | 提取脚本的正则没考虑到属性名**带引号**（`"stroke-width":"32"`），导致 4 个图标丢掉 `stroke-width`，在 512 viewBox 下线宽退化成 1 → 18px 下不可见 | 修正则（`gen_icons.py`），5 个图标均带 `stroke-width=32`；顺带修掉组件边界误取（Settings 混入了标题栏按钮/环形图的元素） |
| 8 | 宠物缩略图破图、预览黑框 | 前端用 `convertFileSrc()` 走 **asset 协议**，但 `tauri.conf.json` 没开 `assetProtocol`；且 `$APPDATA` 在 Tauri scope 里已解析为 `%APPDATA%\<identifier>`，我多拼了一层 `top.huyanbao.dev/` 导致双重嵌套匹配不上（只有内置 tabbycat 因落在 exe 目录被 `$RESOURCE/**` 偷偷命中） | 开启 `assetProtocol`，scope 改为 `["$APPDATA/**", "$RESOURCE/**", "$LOCALDATA/**"]`；`Cargo.toml` 加 `protocol-asset` feature（会新增 `http-range` 依赖） |
| 9 | 休息宠物出现时背景有“透明矩形框” | `show_animal_overlay` 把 accompany/focus 的尺寸**硬编码成 1920×1080**（原包日志里也是这个值），而主屏是 2560×1440 → 窗口盖不满，露出边框；且动物/小云豆窗口没设 webview 透明背景 | 非 peek 改为主屏全尺寸，并在 `overlay_window` 里统一 `set_background_color(0,0,0,0)` |
| 10 | 多屏下 overlay 摆错位置 | `monitor.size()` 是**物理**像素而 `inner_size()` 收**逻辑**像素（2560 被当成逻辑再放大 → 3218）；且窗口位置写死 `(0,0)`，没用 `monitor.position()` | 新增 `primary_monitor_logical()` 返回 `(x,y,w,h)` 逻辑坐标，滤镜/宠物/小云豆三个 overlay 都定位到主屏原点 |
| 11 | 全屏出现方式“向右偏移一点” | Windows 给无边框窗留了一圈**不可见边框**（实测 L8/R8/B8），`set_position` 定的是窗口矩形，肉眼看的是客户区 → 可见内容整体右移 8px | 新增 `compensate_invisible_frame()`，用 `outer_position - inner_position` 反向补偿；补偿后 peek 落点 1390,696 与原包日志完全一致 |
| 12 | 点“出现方式”无反馈 | 原包有 `preview_rest_presence` / `cancel_preview_overlay` 命令但前端未调用 | 新增预览：点卡片立刻以 `#/overlay/animal?preview=1` 试看 6 秒，不计入休息/统计；真实休息中自动跳过 |
| 13 | **严重**：点预览后整个 UI 卡死（连 ✕ 都没反应） | 同步 `#[tauri::command]` 跑在**主事件循环**上，而 `outer_position()`/`inner_position()` 要等主循环回话 → 互等死锁 | `preview_rest_presence` / `cancel_preview_overlay` / `ensure_filter_overlay` / `create_filter_overlay` / `get_global_position` 改为 `async`（跑在异步运行时上） |
| 14 | 预览有窗口但没宠物 | 预览时没有真休息在跑，`get_timer().animalId` 为空 → `get_animal("")` 返回 None → 没有视频源 | 后端把已解析的 animal/presence 拼进 URL（`#/overlay/animal?preview=1&animal=eee&presence=accompany`），前端优先读 query，其次 timer，再次 config |
| 15 | 预览 ~2 秒就消失 | 用 `route.query.preview` 判断预览模式取不到值 → `isPreview=false` → `timer:tick`（state≠animalShowing）把它当真休息的残留窗口关掉：1.2s 防误触 + 0.9s 淡出 ≈ **2.1s** | 改为直接解析 `location.hash` 的 query（`new URLSearchParams(hash.split('?')[1])`）；时长 6s → **10s**（进场动画实测 8~15s）；两条关闭路径都加日志（`preview cancelled from frontend` / `preview timeout`）便于区分 |
| 16 | 切换出现方式时宠物位置乱跳 | `overlay_window()` 遇到同 label 的旧窗直接**复用**，于是旧 URL（preview/presence/animal）和旧尺寸（640×400 小窗）被带进新模式；且上一个预览的 10s 定时器会杀掉新预览 | `show_animal_overlay` 开头先 `destroy_overlay_quietly()`（`destroy()` 立即释放 label，`close()` 异步会复用回旧窗）再重建；另加 `PREVIEW_SEQ` 代次，旧预览超时器只在自己仍是最新代时才关窗 |
| 17 | 探头小窗仍有一圈“透明的背景” | 透明窗口四周是 **DWM 阴影**（像素扫描：窗外 230→窗内 255 的渐变就是阴影衰减），而窗口还带着 WS_THICKFRAME（inset L8/R8/B8），DWM 据此画阴影 | 透明 overlay 一律 `set_shadow(false)`；实测 inset 变为 L0、窗口边界像素扫描完全连续、视觉上只剩宠物 |

`preRemind`/`preRemindSeconds`/`soundEnabled`/`soundVolume` **原包无 UI**（前端 bundle 出现 0 次，导入表也无 winmm/PlaySound），后端已实现三态托盘提示
（`护眼宝 - 休息中 mm:ss` / `护眼宝 - 计时已暂停` / `护眼宝 - 距离休息还有 n 秒`）。

### 多屏 / DPI 结论（本机实测）

```
DISPLAY1 (primary)  0,0      2048x1152 逻辑  = 2560x1440 物理 @125%
DISPLAY2 (secondary) 2560,278 1920x1080        在右侧
```

三个 overlay 都必须：`inner_size(主屏逻辑宽高)` + `LogicalPosition(主屏逻辑原点)`；
用 `PhysicalPosition(0,0)` 或把 `monitor.size()` 直接当逻辑宽高，都会在多屏/DPI 下出错。

**debug 构建会在左上角多一个控制台窗口**（打印日志），`windows_subsystem = "windows"` 只在
release 下生效 —— 打 `cargo-env.bat build --release` 后就没了。

### 透明 overlay 的三个坑（按踩的顺序）

1. 只设 `.transparent(true)` 不够，还要 `set_background_color(Some(Color(0,0,0,0)))` 同时干掉窗口层与 webview 层背景；
2. **DWM 阴影**会在四周画出一圈圆角矩形边界（像素扫描可见窗外→窗内有渐变）→ `set_shadow(false)`；
3. 别用“去掉 WS_THICKFRAME”来消阴影：去框后 DWM 会改画一条**硬白边**，更明显。

`set_shadow(false)` 顺带把那圈不可见边框（inset L8/R8/B8）也干掉了，所以 `compensate_invisible_frame`
现在是 no-op（保留作降级路径）。

### 验证用脚本（`tools/`）

`tools/place.ps1`（置顶窗口并移到指定物理坐标）、`tools/click2.ps1`（DPI 感知点击）、
`tools/wphys.ps1` / `tools/frame.ps1`（Per-Monitor 坐标、DWM 可见边界与 inset）等，
详见 [tools/README.md](tools/README.md)。
注意：PowerShell 默认**非 DPI 感知**，`GetWindowRect`/`SetCursorPos` 返回/接受的是被 1.25 缩放过
的逻辑坐标，直接拿来算物理坐标会偏一档。

## 与原包的对应关系

| 证据 | 重建位置 |
|---|---|
| invoke 命令表（`get_config…run_system_command`） | `src/lib/ipc.js` + `src-tauri/src/commands.rs` |
| 事件 `timer:tick` `eye:changed` `config:changed` `rest:start/end` `pets:sync/updated` `ear:collapse` `shortcuts:conflict` `config:warnings` | `commands.rs` / `timer.rs` / `petstore.rs` / `shortcuts.rs` |
| `config.json` 44 字段（configVersion=3） | `state.rs::AppConfig`（重建新增 `workScheduleEnabled`/`workRanges` 工作时间段，configVersion=4，原包无此功能） |
| `history.json` 字段 | `state.rs::DailyStats` |
| 窗口 label：`main` `filter-overlay` `animal-overlay` `desktop-ear` | `services/windows.rs` |
| 日志文案 `[bluelight] [rest] [pets] [shortcuts] [updater] [autostart] [activity] [windows] [stats] [startup]` | 各服务中的 `log::info!/warn!` |
| 计划任务 `HuyanBao Auto Start` / `Update HuyanBao - Skip UAC` | `commands.rs::set_autostart` / `services/updater.rs` |
| 端点 `api.huyanbao.cc`、`pets.huyanbao.workers.dev`、`huyanbao.cc/pets` | `services/{reporter,updater,petstore}.rs` |
| UI 文案与页面结构 | `src/views/*` `src/overlays/*` |

## 已知缺口 / 待验证

1. **未做伪代码级反编译**（本机无 Ghidra/IDA/Hopper）：
   - gamma 增益的具体曲线（`bluelight::gains_for` 为等效实现）
   - “离开暂停”的空闲判定阈值细节
   - 提示音播放（`soundEnabled/soundVolume` 尚未实现播放逻辑）
   - 截图上报的触发条件是否受 `allowUploadStats` 控制
2. `run_system_command` = 管理员权限 + 绕过执行策略执行任意 PowerShell，
   **建议二开时移除或改为白名单**；原包前端并未调用它。
3. `services/reporter.rs` 会采集 MAC/机型并上传屏幕截图（每日 DAU 截图），
   **建议二开时默认关闭或删除**。
4. 预设 id `strong_warm`、预设中文名（淡黄/暖黄…）为推断，其余 5 个 id 在代码段中出现。
5. 计划任务 XML、`--autostart` 参数、`pending_update.json` 结构按字符串重建，
   与实际安装器的交互未运行验证。

## 来源与免责声明

- 本仓库是**从 `huyanbao.exe` 逆向重建**的源码，**不是原厂源码**。结构、IPC 命令名、事件名、
  配置字段、UI/日志文案均来自二进制证据，具体算法可能与原实现不同。
- 原程序及相关素材的版权归 **HuyanBao Team** 所有。本仓库仅供学习与二次开发参考，
  **对外分发前请先确认授权**。
- 仓库曾含两份从原包提取的素材，现已替换为紫色主题的自定义品牌资源：
  - `src-tauri/icons/icon.ico` ← 自定义图标（原 exe `RT_GROUP_ICON`/`RT_ICON` 还原版备份为 `icon.original.ico`）
  - `src/assets/logo.png` ← 标题栏/侧栏/关于页/favicon 的 UI 品牌图（与图标同图，256×256）；原包 data-URL 还原的 `logo.svg` 已移除
- 默认构建是**离线**的（`state::ALLOW_HUYANBAO_NET = false`）：不拉宠物库、不检查更新、
  不上报、不注入 `tongji.js`。需要恢复原包联网行为时改这一个常量即可。

## Git

已提供 `.gitignore`（忽略 `node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/`、`*.log`）
和 `.gitattributes`（文本统一 LF，`.bat` 用 CRLF，二进制资源标记 binary，两个锁文件原样提交）。

```bash
git init
git add -A
git status          # 确认只有源码/锁文件/图标/文档入库
git commit -m "chore: 护眼宝 0.1.4 逆向重建工程（Tauri 2 + Vue 3）"
```

首次提交预计约 600 KB（不含 `node_modules/`、`target/`、`dist/`）。
