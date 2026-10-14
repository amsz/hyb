# 验证脚本（Win32 + PowerShell）

反向重建过程中用来**驱动 UI / 读取真实窗口几何**的脚本，与业务代码无关，
仅用于人工验证。全部只读操作目标程序，不修改其数据。

| 脚本 | 作用 |
|---|---|
| `wphys.ps1` | 以 **Per-Monitor DPI 感知**枚举本进程可见窗口，输出**物理坐标**的 `x,y,宽x高` |
| `frame.ps1` | 同时输出 `GetWindowRect`（窗口矩形）与 `DwmGetWindowAttribute(EXTENDED_FRAME_BOUNDS)`（可见区域），用于算不可见边框 inset |
| `place.ps1` | 把目标窗口置顶并移到指定物理坐标（便于截图对位） |
| `click2.ps1` | DPI 感知的鼠标点击（`SetCursorPos` + `mouse_event`），坐标为**物理**值 |
| `scroll.ps1` | 在指定位置发送滚轮（负 delta = 向下滚动） |
| `poll.ps1` | 轮询本进程窗口标题里是否含 `overlay`，用于测量预览窗口存活时长 |

## 为什么必须显式做 DPI 感知

Windows 对非感知进程做**坐标虚拟化**：PowerShell 默认非 DPI 感知，
`GetWindowRect` 返回的是被缩放过的逻辑坐标，`SetCursorPos` 收的也是逻辑坐标。
在 125% 缩放下直接混用会整体偏一档（实测点击偏移约 43×101 px）。

三个脚本里都有这一行：

```powershell
SetThreadDpiAwarenessContext(new IntPtr(-4))   # DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
```

## 用法示例

```powershell
# 看窗口几何（物理坐标 + inset）
powershell -ExecutionPolicy Bypass -File tools/wphys.ps1
powershell -ExecutionPolicy Bypass -File tools/frame.ps1

# 把窗口摆到 (300,120) 并置顶，然后点宠物页
powershell -ExecutionPolicy Bypass -File tools/place.ps1 -X 300 -Y 120
powershell -ExecutionPolicy Bypass -File tools/click2.ps1 -X 390 -Y 436

# 测预览窗口存活时长
powershell -ExecutionPolicy Bypass -File tools/poll.ps1 -Seconds 15
```

> 这些脚本按本机双屏 + 125% 缩放调过坐标；换机器要按 `wphys.ps1` 的输出重新对位。
