<#
.SYNOPSIS
    Self-contained dev build for VirtualBow (GUI / lib / tests).

.DESCRIPTION
    Sets PATH to the MSYS2 mingw64 toolchain BEFORE invoking cmake/ninja, so
    that moc.exe / cc1plus.exe / ninja.exe can resolve their runtime DLLs.
    Without this prelude, a stray C:\MinGW\bin (or the mere absence of
    C:\msys64\mingw64\bin from PATH) makes Windows fail tool launches with
    STATUS_DLL_NOT_FOUND (0xc0000135) — which CMake/ninja surface only as
    silent "exit 1" and "Test run of moc executable failed", leaving no clue
    about the real cause.

    Use this script (or the steps it runs) any time you build from a fresh
    PowerShell session.

.PARAMETER Target
    The ninja target to build. Default: virtualbow-gui.

.PARAMETER Reconfigure
    Force `cmake .` reconfigure before building.

.PARAMETER Clean
    Remove the build/ directory entirely and start fresh (implies reconfigure).
#>
param(
    [string]$Target = "virtualbow-gui",
    [switch]$Reconfigure,
    [switch]$Clean
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ScriptDir  = $PSScriptRoot
$BuildDir   = Join-Path $ScriptDir "build"
$GuiDir     = Join-Path $ScriptDir "gui"
$Msys2Root  = "C:\msys64"
$Mingw64Bin = "$Msys2Root\mingw64\bin"

if (-not (Test-Path $Mingw64Bin)) {
    throw "MSYS2 mingw64 not found at $Mingw64Bin. Install MSYS2 (https://www.msys2.org/) or adjust `$Msys2Root."
}

# Critical: prepend mingw64 AFTER System32 so PowerShell-builtin tools (cmd.exe,
# etc.) keep resolving from Windows, but cmake-spawned compilers find their
# DLL dependencies in mingw64/bin.
$env:PATH = "C:\Windows\System32;C:\Windows;$Mingw64Bin;$Msys2Root\usr\bin;" + $env:PATH

Write-Host "==> Toolchain PATH:" -ForegroundColor Cyan
Write-Host "    $Mingw64Bin"

if ($Clean) {
    if (Test-Path $BuildDir) {
        Write-Host "==> Cleaning $BuildDir" -ForegroundColor Cyan
        Remove-Item -Recurse -Force $BuildDir
    }
    $Reconfigure = $true
}

if (-not (Test-Path $BuildDir)) {
    New-Item -ItemType Directory $BuildDir | Out-Null
    $Reconfigure = $true
}

Push-Location $BuildDir
try {
    if ($Reconfigure -or -not (Test-Path (Join-Path $BuildDir "build.ninja"))) {
        Write-Host "==> Configuring with CMake (Ninja, Release)" -ForegroundColor Cyan
        & cmake -G Ninja -DCMAKE_BUILD_TYPE=Release $GuiDir
        if ($LASTEXITCODE -ne 0) { throw "cmake configure failed (exit $LASTEXITCODE)" }
    }

    # The CMake build does NOT track the Rust static library
    # (libvirtualbow_ffi.a) as a ninja dependency. Cargo's own incremental
    # rebuild is correct, but ninja can't see when the .a changed and so
    # may skip relinking the GUI even though the solver was updated.
    # To keep dev builds honest, always invoke cargo first and, if the
    # resulting .a is newer than the GUI executable, force a relink by
    # removing the stale exe before running ninja.
    Write-Host "==> Updating Rust static library" -ForegroundColor Cyan
    Push-Location (Join-Path $ScriptDir "rust")
    try {
        # Cargo writes progress to stderr. Under $ErrorActionPreference=Stop,
        # PowerShell would otherwise convert that to a NativeCommandError and
        # abort the script. Suspend the strict policy just for this call.
        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            & cargo build --release -p virtualbow_ffi --target x86_64-pc-windows-gnu 2>&1 | Out-Host
        }
        finally { $ErrorActionPreference = $prev }
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
    }
    finally {
        Pop-Location
    }

    $ffi = Join-Path $ScriptDir "rust\target\x86_64-pc-windows-gnu\release\libvirtualbow_ffi.a"
    $gui = Join-Path $BuildDir "application\virtualbow-gui.exe"
    if ((Test-Path $ffi) -and (Test-Path $gui)) {
        if ((Get-Item $ffi).LastWriteTime -gt (Get-Item $gui).LastWriteTime) {
            Write-Host "==> Rust lib is newer than GUI exe, forcing relink" -ForegroundColor Yellow
            Remove-Item $gui -Force
        }
    }

    Write-Host "==> Building target: $Target" -ForegroundColor Cyan
    & ninja $Target
    if ($LASTEXITCODE -ne 0) { throw "ninja build failed (exit $LASTEXITCODE)" }

    Write-Host "==> Build OK" -ForegroundColor Green
}
finally {
    Pop-Location
}
