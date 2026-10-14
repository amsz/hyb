<#
.SYNOPSIS
  护眼宝 —— 打包脚本：构建出可直接运行的 exe（+ 便携 zip）。

.DESCRIPTION
  1. 前端  : npm run build            -> dist/（release 构建会把它内嵌进 exe）
  2. 后端  : src-tauri\cargo-env.bat build --release  -> target\release\huyanbao.exe
  3. 组装  : release\huyanbao-<版本>-win-x64\huyanbao.exe + 使用说明.txt
  4. 压缩  : release\huyanbao-<版本>-win-x64-portable.zip + SHA256

  运行前置：已安装 WebView2 Runtime（Win10/11 与 Edge 一般自带）。

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File package.ps1
  powershell -ExecutionPolicy Bypass -File package.ps1 -SkipFrontend
  powershell -ExecutionPolicy Bypass -File package.ps1 -Production
  powershell -ExecutionPolicy Bypass -File package.ps1 -Installer
#>
[CmdletBinding()]
param(
    # 按“正式版”配置打包：identifier / IS_DEV / ALLOW_HUYANBAO_NET 三处切回原包值（构建后自动还原）
    [switch]$Production,
    # 额外用 tauri build 出 NSIS 安装包（需要联网拉 NSIS，失败不阻断便携包）
    [switch]$Installer,
    [switch]$SkipFrontend,
    [switch]$SkipBackend,
    [string]$OutDir = "release"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Tauri = Join-Path $Root "src-tauri"
$CargoEnv = Join-Path $Tauri "cargo-env.bat"

function Write-Step($msg) { Write-Host "`n==> $msg" -ForegroundColor Cyan }
function Write-Ok($msg)    { Write-Host "    OK  $msg" -ForegroundColor Green }
function Fail($msg)        { Write-Host "    FAIL  $msg" -ForegroundColor Red; exit 1 }

# ---------------------------------------------------------------- 版本
# 注意：PowerShell 5.1 默认用系统 ANSI(GBK) 读文件，package.json 含中文会被截断 -> 必须显式 UTF8
$pkg = [System.IO.File]::ReadAllText((Join-Path $Root "package.json"), [Text.Encoding]::UTF8) | ConvertFrom-Json
$Version = $pkg.version
if (-not $Version) { Fail "cannot read version from package.json" }

$StageName = "huyanbao-$Version-win-x64"
$Stage = Join-Path (Join-Path $Root $OutDir) $StageName
$Zip   = Join-Path (Join-Path $Root $OutDir) "$StageName-portable.zip"

Write-Host "版本: $Version    配置: $(if ($Production) {'Production'} else {'Dev (离线/独立标识)'})" -ForegroundColor Yellow

# ---------------------------------------------------------------- 1. 前端
if (-not $SkipFrontend) {
    Write-Step "前端构建 (vite)"
    Push-Location $Root
    try {
        if (-not (Test-Path (Join-Path $Root "node_modules"))) {
            npm install --no-audit --no-fund
            if ($LASTEXITCODE -ne 0) { Fail "npm install failed" }
        }
        npm run build
        if ($LASTEXITCODE -ne 0) { Fail "vite build failed" }
    } finally { Pop-Location }
    if (-not (Test-Path (Join-Path $Root "dist\index.html"))) { Fail "dist/index.html missing" }
    Write-Ok "dist/index.html"
} else {
    Write-Step "跳过前端构建 (-SkipFrontend)"
    if (-not (Test-Path (Join-Path $Root "dist\index.html"))) { Fail "dist/index.html missing; run without -SkipFrontend" }
}

# ---------------------------------------------------------------- 2. 后端（可能切生产配置）
$targets = @(
    @{ File = (Join-Path $Tauri "tauri.conf.json"); Old = '"identifier": "top.huyanbao.dev"'; New = '"identifier": "top.huyanbao.desktop"' },
    @{ File = (Join-Path $Tauri "src\state.rs");    Old = 'pub const APP_DIR_NAME: &str = "top.huyanbao.dev";';   New = 'pub const APP_DIR_NAME: &str = "top.huyanbao.desktop";' },
    @{ File = (Join-Path $Tauri "src\state.rs");    Old = 'pub const IS_DEV: bool = true;';                        New = 'pub const IS_DEV: bool = false;' },
    @{ File = (Join-Path $Tauri "src\state.rs");    Old = 'pub const ALLOW_HUYANBAO_NET: bool = false;';           New = 'pub const ALLOW_HUYANBAO_NET: bool = true;' }
)

function Set-ProductionFlags {
    foreach ($t in $targets) {
        $content = [System.IO.File]::ReadAllText($t.File, [Text.Encoding]::UTF8)
        if (-not $content.Contains($t.Old)) { throw "pattern not found in $($t.File): $($t.Old)" }
        [System.IO.File]::WriteAllText($t.File, ($content.Replace($t.Old, $t.New)), (New-Object Text.UTF8Encoding $false))
    }
}
function Restore-Source {
    foreach ($t in $targets) {
        $content = [System.IO.File]::ReadAllText($t.File, [Text.Encoding]::UTF8)
        if ($content.Contains($t.New)) {
            [System.IO.File]::WriteAllText($t.File, ($content.Replace($t.New, $t.Old)), (New-Object Text.UTF8Encoding $false))
        }
    }
}

if (-not $SkipBackend) {
    Write-Step "后端构建 (cargo build --release)"
    if (-not (Test-Path $CargoEnv)) { Fail "missing src-tauri\cargo-env.bat" }

    $swapped = $false
    try {
        if ($Production) {
            Write-Step "切换为正式版配置 (identifier / IS_DEV / ALLOW_HUYANBAO_NET)"
            Set-ProductionFlags
            $swapped = $true
            Write-Ok "3 处配置已切换（构建结束后自动还原）"
        }

        Push-Location $Tauri
        try {
            # --features custom-protocol 是必须的：它决定 Tauri 把前端内嵌而不是去连 devUrl
            & cmd /c "`"$CargoEnv`" build --release --features custom-protocol"
            if ($LASTEXITCODE -ne 0) { Fail "cargo build --release failed (exit $LASTEXITCODE)" }
        } finally { Pop-Location }

        $Exe = Join-Path $Tauri "target\release\huyanbao.exe"
        if (-not (Test-Path $Exe)) { Fail "target\release\huyanbao.exe not found" }
        Write-Ok "$([math]::Round((Get-Item $Exe).Length/1MB,1)) MB"
    }
    finally {
        if ($swapped) {
            Restore-Source
            Write-Step "已还原源码中的开发配置"
        }
    }
}

$ExeSrc = Join-Path $Tauri "target\release\huyanbao.exe"
if (-not (Test-Path $ExeSrc)) { Fail "target\release\huyanbao.exe not found (use without -SkipBackend first)" }

# ---------------------------------------------------------------- 3. 组装便携目录
Write-Step "组装便携包 $StageName"
if (Test-Path $Stage) { Remove-Item $Stage -Recurse -Force }
New-Item -ItemType Directory -Path $Stage -Force | Out-Null
Copy-Item $ExeSrc (Join-Path $Stage "huyanbao.exe")

$readme = @"
护眼宝 $Version —— 便携版（win-x64）
================================================

【运行】
  双击 huyanbao.exe 即可，无需安装。
  需要 WebView2 Runtime（Win10/11 与 Edge 一般自带）。

【数据目录】
  配置/统计/宠物   %APPDATA%\top.huyanbao.desktop\
  日志/WebView 缓存 %LOCALAPPDATA%\top.huyanbao.desktop\

【说明】
  本包由 huyanbao.exe 0.1.4 逆向重建而来，版权归原 HuyanBao Team。
  若以 -Production 参数打包，则 identifier / 数据目录 / 联网策略与原包一致；
  默认（Dev）构建使用 top.huyanbao.dev 标识且完全离线，不会与 huyanbao.cc 通信。
"@
[System.IO.File]::WriteAllText((Join-Path $Stage "使用说明.txt"), $readme, (New-Object Text.UTF8Encoding $false))

# ---------------------------------------------------------------- 4. 压缩 + 校验
Write-Step "压缩"
if (Test-Path $Zip) { Remove-Item $Zip -Force }
Compress-Archive -Path $Stage -DestinationPath $Zip -CompressionLevel Optimal
$hash = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
"$hash  $(Split-Path $Zip -Leaf)" | Set-Content "$Zip.sha256" -Encoding ascii

# ---------------------------------------------------------------- 5. 可选 NSIS 安装包
if ($Installer) {
    Write-Step "NSIS 安装包 (tauri build) —— 需要联网拉取 NSIS"
    Push-Location $Root
    try {
        npx tauri build
        if ($LASTEXITCODE -ne 0) {
            Write-Host "    WARN  tauri build 失败（离线环境常见），便携包不受影响" -ForegroundColor Yellow
        } else {
            Get-ChildItem (Join-Path $Root "src-tauri\bundle\nsis") -Filter *.exe -ErrorAction SilentlyContinue |
                ForEach-Object { Write-Ok "安装包: $($_.FullName)" }
        }
    } finally { Pop-Location }
}

# ---------------------------------------------------------------- 汇总
Write-Host ""
Write-Host "打包完成" -ForegroundColor Green
"  便携目录 : $Stage"
"  便携压缩 : $Zip ($([math]::Round((Get-Item $Zip).Length/1MB,1)) MB)"
"  SHA256   : $hash"
"  配置     : $(if ($Production) {'Production（已还原源码）'} else {'Dev（离线 / top.huyanbao.dev）'})"
