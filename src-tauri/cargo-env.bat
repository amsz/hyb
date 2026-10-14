@echo off
setlocal EnableExtensions EnableDelayedExpansion
REM ------------------------------------------------------------------
REM Build entry (Windows / MSVC).
REM
REM 1) vcvarsall.bat cannot be used on this setup: VS18 points
REM    Microsoft.VCToolsVersion.default.txt at 14.51.x, which is not fully
REM    installed here, so vcvarsall reports success but injects no MSVC
REM    paths. We therefore assemble the environment ourselves from the
REM    newest *complete* toolset plus the Windows SDK.
REM 2) The MSVC bin dir must come FIRST on PATH, otherwise the GNU
REM    link.exe shipped with Git for Windows / coreutils shadows it and
REM    every crate that links fails with "link: extra operand".
REM ------------------------------------------------------------------

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
set "VSDIR="
if exist "%VSWHERE%" (
  for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -property installationPath 2^>nul`) do set "VSDIR=%%i"
)
if not defined VSDIR set "VSDIR=%ProgramFiles(x86)%\Microsoft Visual Studio\18\BuildTools"

REM newest toolset directory that really contains bin\HostX64\x64\link.exe
set "MSVCDIR="
for /f "delims=" %%d in ('dir /b /ad /o-n "%VSDIR%\VC\Tools\MSVC" 2^>nul') do (
  if exist "%VSDIR%\VC\Tools\MSVC\%%d\bin\HostX64\x64\link.exe" if not defined MSVCDIR set "MSVCDIR=%%d"
)
if not defined MSVCDIR (
  echo [cargo-env] no complete MSVC toolset under "%VSDIR%\VC\Tools\MSVC" 1>&2
  exit /b 1
)
set "VC=%VSDIR%\VC\Tools\MSVC\!MSVCDIR!"

set "SDKDIR=%ProgramFiles(x86)%\Windows Kits\10"
set "SDKVER="
for /f "delims=" %%d in ('dir /b /ad /o-n "%SDKDIR%\Include" 2^>nul') do if not defined SDKVER set "SDKVER=%%d"
if not defined SDKVER (
  echo [cargo-env] no Windows SDK under "%SDKDIR%\Include" 1>&2
  exit /b 1
)

set "PATH=!VC!\bin\HostX64\x64;%SDKDIR%\bin\!SDKVER!\x64;%PATH%"
set "INCLUDE=!VC!\include;%SDKDIR%\Include\!SDKVER!\ucrt;%SDKDIR%\Include\!SDKVER!\shared;%SDKDIR%\Include\!SDKVER!\um;%SDKDIR%\Include\!SDKVER!\winrt"
set "LIB=!VC!\lib\x64;%SDKDIR%\Lib\!SDKVER!\ucrt\x64;%SDKDIR%\Lib\!SDKVER!\um\x64"

cd /d "%~dp0"
cargo %*
