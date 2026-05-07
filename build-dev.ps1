<#
.SYNOPSIS
    Self-contained dev build for VirtualBow (GUI / lib / tests).

.DESCRIPTION
    Auto-detects MSYS2, installs any missing mingw64 packages and Rust
    targets, then configures and builds with CMake + Ninja.

    Sets PATH to the MSYS2 mingw64 toolchain BEFORE invoking cmake/ninja, so
    that moc.exe / cc1plus.exe / ninja.exe can resolve their runtime DLLs.
    Without this prelude, a stray C:\MinGW\bin (or the mere absence of
    <msys2>\mingw64\bin from PATH) makes Windows fail tool launches with
    STATUS_DLL_NOT_FOUND (0xc0000135) — which CMake/ninja surface only as
    silent "exit 1" and "Test run of moc executable failed", leaving no clue
    about the real cause.

.PARAMETER Target
    The ninja target to build. Default: virtualbow-gui.

.PARAMETER Reconfigure
    Force `cmake .` reconfigure before building.

.PARAMETER Clean
    Remove the build/ directory entirely and start fresh (implies reconfigure).

.PARAMETER Msys2Root
    Override MSYS2 install root. Auto-detected if omitted.

.PARAMETER NoInstall
    Skip the prerequisite-install step (assume packages and Rust target
    are already present).
#>
param(
    [string]$Target = "virtualbow-gui",
    [switch]$Reconfigure,
    [switch]$Clean,
    # MSYS2 install root. Resolution order:
    #   1. -Msys2Root command-line argument
    #   2. $env:MSYS2_ROOT environment variable
    #   3. Auto-detect across common install locations
    [string]$Msys2Root,
    [switch]$NoInstall
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-Msys2Root([string]$Override) {
    if ($Override) { return $Override }
    if ($env:MSYS2_ROOT) { return $env:MSYS2_ROOT }
    $candidates = @(
        "C:\msys64",
        "D:\msys64",
        "$env:SystemDrive\msys64",
        "$env:USERPROFILE\msys64",
        "$env:ProgramFiles\msys64",
        "$env:LOCALAPPDATA\msys64"
    ) | Where-Object { $_ } | Select-Object -Unique
    foreach ($c in $candidates) {
        if (Test-Path (Join-Path $c "mingw64\bin\gcc.exe")) { return $c }
    }
    # Last resort: search PATH for gcc.exe and walk up to the MSYS2 root.
    $gcc = Get-Command gcc.exe -ErrorAction SilentlyContinue
    if ($gcc) {
        $dir = Split-Path -Parent $gcc.Source
        # gcc lives in <root>\mingw64\bin
        $root = Split-Path -Parent (Split-Path -Parent $dir)
        if (Test-Path (Join-Path $root "mingw64\bin\gcc.exe")) { return $root }
    }
    throw "Could not locate MSYS2. Pass -Msys2Root, set `$env:MSYS2_ROOT, or install to a standard location."
}

$ScriptDir  = $PSScriptRoot
$BuildDir   = Join-Path $ScriptDir "build"
$GuiDir     = Join-Path $ScriptDir "gui"
$Msys2Root  = Resolve-Msys2Root $Msys2Root
$Mingw64Bin = Join-Path $Msys2Root "mingw64\bin"

if (-not (Test-Path (Join-Path $Mingw64Bin "gcc.exe"))) {
    throw "MSYS2 mingw64 toolchain not found at $Mingw64Bin. Install MSYS2 (https://www.msys2.org/) and run: pacman -S mingw-w64-x86_64-toolchain."
}

# Critical: prepend mingw64 AFTER System32 so PowerShell-builtin tools (cmd.exe,
# etc.) keep resolving from Windows, but cmake-spawned compilers find their
# DLL dependencies in mingw64/bin.
$Msys2UsrBin = Join-Path $Msys2Root "usr\bin"
$env:PATH = "$env:SystemRoot\System32;$env:SystemRoot;$Mingw64Bin;$Msys2UsrBin;" + $env:PATH

Write-Host "==> Toolchain PATH:" -ForegroundColor Cyan
Write-Host "    $Mingw64Bin"

# Required mingw64 packages for a dev build of the GUI + tests.
$RequiredPackages = @(
    "mingw-w64-x86_64-gcc",
    "mingw-w64-x86_64-cmake",
    "mingw-w64-x86_64-ninja",
    "mingw-w64-x86_64-qt6-base",
    "mingw-w64-x86_64-qt6-tools",
    "mingw-w64-x86_64-nlohmann-json",
    "mingw-w64-x86_64-catch"
)

if (-not $NoInstall) {
    Write-Host "==> Verifying MSYS2 mingw64 packages" -ForegroundColor Cyan
    $Pacman = Join-Path $Msys2UsrBin "pacman.exe"
    if (-not (Test-Path $Pacman)) {
        throw "pacman not found at $Pacman. The MSYS2 install at $Msys2Root looks incomplete."
    }
    $installed = & $Pacman -Q 2>$null | ForEach-Object { ($_ -split ' ')[0] }
    $missing = @($RequiredPackages | Where-Object { $_ -notin $installed })
    if ($missing.Count -gt 0) {
        Write-Host "    Installing: $($missing -join ', ')" -ForegroundColor Yellow
        & $Pacman -S --needed --noconfirm @missing
        if ($LASTEXITCODE -ne 0) { throw "pacman install failed (exit $LASTEXITCODE). Run from an elevated/MSYS2 shell, or pass -NoInstall and install manually." }
    } else {
        Write-Host "    All required packages present." -ForegroundColor Green
    }

    # Rust target for the FFI library (GNU ABI matches the MSYS2 GCC).
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "Rust toolchain not found on PATH. Install from https://rustup.rs and re-run."
    }
    $rustTarget = "x86_64-pc-windows-gnu"
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $installedTargets = @(& rustup target list --installed 2>$null)
        if (-not ($installedTargets | Where-Object { $_ -eq $rustTarget })) {
            Write-Host "    Adding Rust target: $rustTarget" -ForegroundColor Yellow
            & rustup target add $rustTarget 2>&1 | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "rustup target add failed (exit $LASTEXITCODE)" }
        }
    }
    finally { $ErrorActionPreference = $prev }
}

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
