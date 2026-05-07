<#
.SYNOPSIS
    Builds a self-contained Windows portable package of VirtualBow.

.DESCRIPTION
    This script:
      1. Installs required MSYS2 mingw64 packages (Qt6, GCC, Ninja, etc.)
      2. Compiles the Rust FFI library for the x86_64-pc-windows-gnu target
      3. Configures and builds the C++ GUI with CMake + Ninja
      4. Runs windeployqt6 to bundle Qt6 plugins
      5. Copies all required MinGW runtime DLLs
      6. Copies ffmpeg.exe
      7. Produces a portable ZIP: virtualbow-portable-<version>.zip

.PARAMETER SkipRustBuild
    Skip rebuilding the Rust libraries (use existing artifacts).

.PARAMETER SkipCppBuild
    Skip rebuilding the C++ GUI (use existing artifacts).

.PARAMETER OutputDir
    Directory where the portable ZIP will be written. Defaults to the workspace root.
#>
param(
    [switch]$SkipRustBuild,
    [switch]$SkipCppBuild,
    [string]$OutputDir = $PSScriptRoot,
    # MSYS2 install root. Resolution order:
    #   1. -Msys2Root command-line argument
    #   2. $env:MSYS2_ROOT environment variable
    #   3. Auto-detect across common install locations
    [string]$Msys2Root
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
        if (Test-Path (Join-Path $c "usr\bin\pacman.exe")) { return $c }
    }
    $gcc = Get-Command gcc.exe -ErrorAction SilentlyContinue
    if ($gcc) {
        $dir = Split-Path -Parent $gcc.Source
        $root = Split-Path -Parent (Split-Path -Parent $dir)
        if (Test-Path (Join-Path $root "usr\bin\pacman.exe")) { return $root }
    }
    throw "Could not locate MSYS2. Pass -Msys2Root, set `$env:MSYS2_ROOT, or install to a standard location."
}

# ── Configuration ──────────────────────────────────────────────────────────────

$ScriptDir    = $PSScriptRoot
$GuiDir       = Join-Path $ScriptDir "gui"
$RustDir      = Join-Path $ScriptDir "rust"
$BuildDir     = Join-Path $ScriptDir "build"
$AppDir       = Join-Path $BuildDir "application"
$Msys2Root    = Resolve-Msys2Root $Msys2Root
$Mingw64Bin   = Join-Path $Msys2Root "mingw64\bin"
$Msys2UsrBin  = Join-Path $Msys2Root "usr\bin"
$Pacman       = Join-Path $Msys2UsrBin "pacman.exe"
$RustTarget   = "x86_64-pc-windows-gnu"
$AppVersion   = "0.11.0"

# MinGW runtime DLLs required at runtime (discovered via objdump analysis)
# ── Helpers ────────────────────────────────────────────────────────────────────

function Write-Step([string]$Message) {
    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

function Invoke-Command-Required([string]$Executable, [string[]]$Arguments, [string]$WorkDir = $PWD) {
    # Some native tools (rustup, cargo) write informational messages to stderr.
    # Under $ErrorActionPreference='Stop', merging 2>&1 turns those into
    # terminating NativeCommandError exceptions even when exit code is 0.
    # Temporarily relax error handling around the invocation.
    #
    # Output is streamed line-by-line via Write-Host so long-running commands
    # (cargo build, cmake, ninja) show progress live instead of staring at a
    # frozen console for minutes.
    Write-Host "  > $Executable $($Arguments -join ' ')" -ForegroundColor DarkGray
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $Executable @Arguments 2>&1 | ForEach-Object { Write-Host $_ }
        $exit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prev
    }
    if ($exit -ne 0) {
        throw "Command failed (exit $exit): $Executable $($Arguments -join ' ')"
    }
}

# Discovers all DLLs needed by the app binaries (recursively via ldd) that
# live in the MSYS2 mingw64 bin directory but are not yet in $AppDir.
# Excludes ffmpeg codec DLLs (av*, sw*, xvidcore) to keep the scan focused;
# those are handled separately by the ffmpeg DLL deploy step.
function Get-MissingMingwDlls([string]$ScanDir, [string]$Mingw64Bin, [string]$Msys2UsrBin) {
    $ldd = Join-Path $Mingw64Bin "ldd.exe"
    if (-not (Test-Path $ldd)) {
        # Fallback: ldd ships in MSYS2 usr/bin
        $ldd = Join-Path $Msys2UsrBin "ldd.exe"
    }
    $missing = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $bins = Get-ChildItem $ScanDir -Recurse -Include "*.dll","*.exe" |
            Where-Object { $_.Name -notmatch "^(ffmpeg|avcodec|avdevice|avfilter|avformat|avutil|swresample|swscale|xvidcore)" }
    foreach ($f in $bins) {
        $lines = & $ldd ($f.FullName) 2>$null
        foreach ($line in $lines) {
            if ($line -match "^\s+(\S+\.dll)\s+=>\s+/mingw64/bin/" ) {
                $dll = $Matches[1]
                if (-not (Test-Path (Join-Path $ScanDir $dll))) {
                    [void]$missing.Add($dll)
                }
            }
        }
    }
    return $missing
}

# ── Step 0: Validate prerequisites ────────────────────────────────────────────

Write-Step "Checking prerequisites"

if (-not (Test-Path $Pacman)) {
    throw "MSYS2 not found at $Msys2Root. Install MSYS2 from https://www.msys2.org first."
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust/Cargo not found. Install from https://rustup.rs first."
}

# ── Step 1: Install MSYS2 packages ────────────────────────────────────────────

Write-Step "Installing/verifying MSYS2 mingw64 packages"

$RequiredPackages = @(
    "mingw-w64-x86_64-gcc",
    "mingw-w64-x86_64-cmake",
    "mingw-w64-x86_64-ninja",
    "mingw-w64-x86_64-qt6-base",
    "mingw-w64-x86_64-qt6-tools",
    "mingw-w64-x86_64-nlohmann-json",
    "mingw-w64-x86_64-catch",
    "mingw-w64-x86_64-ffmpeg"
)

$InstalledOutput = & $Pacman -Q 2>&1
$InstalledPackages = $InstalledOutput | ForEach-Object { ($_ -split " ")[0] }

$ToInstall = @($RequiredPackages | Where-Object { $_ -notin $InstalledPackages })

if ($ToInstall.Count -gt 0) {
    Write-Host "Installing: $($ToInstall -join ', ')" -ForegroundColor Yellow
    & $Pacman -S --noconfirm @ToInstall 2>&1 | ForEach-Object { Write-Host $_ }
    if ($LASTEXITCODE -ne 0) { throw "pacman install failed" }
} else {
    Write-Host "All packages already installed." -ForegroundColor Green
}

# ── Step 2: Set up PATH ────────────────────────────────────────────────────────

Write-Step "Configuring environment PATH"

$env:PATH = "$Mingw64Bin;$Msys2UsrBin;$env:PATH"
Write-Host "Prepended $Mingw64Bin to PATH"

# ── Step 3: Build Rust FFI (GNU target) ───────────────────────────────────────

$RustReleaseDir = Join-Path $RustDir "target\$RustTarget\release"

if ($SkipRustBuild) {
    Write-Step "Skipping Rust build (--SkipRustBuild)"
} else {
    Write-Step "Building Rust FFI library (target: $RustTarget)"

    $installedTargets = rustup target list --installed 2>&1
    if ($installedTargets -notmatch $RustTarget) {
        Write-Host "Adding Rust target $RustTarget ..."
        Invoke-Command-Required rustup @("target", "add", $RustTarget)
    }

    Push-Location $RustDir
    try {
        Write-Host "Running cargo build --release --target $RustTarget ..."
        Invoke-Command-Required cargo @("build", "--release", "--target", $RustTarget, "-p", "virtualbow_ffi")
    } finally {
        Pop-Location
    }
}

$ffiLib = Join-Path $RustReleaseDir "libvirtualbow_ffi.a"
if (-not (Test-Path $ffiLib)) {
    throw "Expected Rust FFI library not found: $ffiLib"
}
Write-Host "Rust FFI library: $ffiLib" -ForegroundColor Green

# ── Step 4: CMake configure + build ───────────────────────────────────────────

if ($SkipCppBuild) {
    Write-Step "Skipping C++ build (--SkipCppBuild)"
} else {
    Write-Step "Configuring CMake"

    New-Item -ItemType Directory -Path $BuildDir -Force | Out-Null
    $CacheFile = Join-Path $BuildDir "CMakeCache.txt"
    if (Test-Path $CacheFile) { Remove-Item $CacheFile -Force }
    $CmakeFilesDir = Join-Path $BuildDir "CMakeFiles"
    if (Test-Path $CmakeFilesDir) { Remove-Item $CmakeFilesDir -Recurse -Force }

    $cmakeArgs = @(
        $GuiDir,
        "-G", "Ninja",
        "-DCMAKE_BUILD_TYPE=Release",
        "-DCMAKE_PREFIX_PATH=$Msys2Root/mingw64",
        "-DCMAKE_C_COMPILER=$Mingw64Bin/gcc.exe",
        "-DCMAKE_CXX_COMPILER=$Mingw64Bin/g++.exe",
        "-DCMAKE_MAKE_PROGRAM=$Mingw64Bin/ninja.exe"
    )

    Push-Location $BuildDir
    try {
        Write-Host "cmake $cmakeArgs"
        Invoke-Command-Required "$Mingw64Bin\cmake.exe" $cmakeArgs $BuildDir
    } finally {
        Pop-Location
    }

    Write-Step "Building C++ GUI"

    Push-Location $BuildDir
    try {
        Invoke-Command-Required "$Mingw64Bin\cmake.exe" @("--build", ".", "-j4", "--target", "virtualbow-gui")
    } finally {
        Pop-Location
    }
}

$GuiExe = Join-Path $AppDir "virtualbow-gui.exe"
if (-not (Test-Path $GuiExe)) {
    throw "Expected GUI executable not found: $GuiExe"
}
Write-Host "GUI executable: $GuiExe" -ForegroundColor Green

# ── Step 5: Run windeployqt6 ──────────────────────────────────────────────────

Write-Step "Running windeployqt6"

$windeployqt = Join-Path $Mingw64Bin "windeployqt6.exe"
if (-not (Test-Path $windeployqt)) {
    throw "windeployqt6.exe not found at $windeployqt"
}

try {
    # windeployqt6 prints its first warning ('Cannot open .../catalogs.json') to
    # stderr BEFORE copying any plugin DLLs. Under $ErrorActionPreference='Stop',
    # the 2>&1 merge would raise a terminating error on that very first line and
    # the surrounding catch would silently swallow a half-complete deploy,
    # leaving the portable build without platforms\qwindows.dll and friends.
    $prevPref = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $windeployqtOutput = & $windeployqt $GuiExe 2>&1
        $deployExit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevPref
    }
    $windeployqtOutput | ForEach-Object { Write-Host $_ }
    if ($deployExit -ne 0) {
        # windeployqt6 from MSYS2 commonly exits 1 because of a missing
        # catalogs.json for translations. That's a known harmless quirk: all
        # plugin DLLs are still copied. Only warn — don't abort.
        Write-Host "  windeployqt6 exited with code $deployExit (treated as non-fatal)" -ForegroundColor Yellow
    }
} catch {
    Write-Host "  (windeployqt6 non-fatal warning suppressed: $_)" -ForegroundColor Yellow
}

# Sanity-check the deployment: the Windows platform plugin is mandatory.
$qwindows = Join-Path $AppDir "platforms\qwindows.dll"
if (-not (Test-Path $qwindows)) {
    throw "windeployqt6 did not deploy platforms\qwindows.dll. Without it the GUI cannot start. Check earlier output."
}
Write-Host "windeployqt6 completed." -ForegroundColor Green

# ── Step 6: Copy MinGW runtime DLLs ──────────────────────────────────────────

Write-Step "Copying MinGW runtime DLLs (auto-discovered via ldd)"

# First pass: copy DLLs required by Qt plugins and main exe
$needed = Get-MissingMingwDlls -ScanDir $AppDir -Mingw64Bin $Mingw64Bin -Msys2UsrBin $Msys2UsrBin
foreach ($dll in ($needed | Sort-Object)) {
    $src = Join-Path $Mingw64Bin $dll
    if (Test-Path $src) {
        try {
            Copy-Item $src (Join-Path $AppDir $dll) -Force
            Write-Host "  Copied: $dll" -ForegroundColor Gray
        } catch {
            Write-Host "  Skipped (in use): $dll" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  Warning: $dll not found in $Mingw64Bin" -ForegroundColor Yellow
    }
}

# Second pass: re-scan now that new DLLs are present (they may have their own deps)
$needed2 = Get-MissingMingwDlls -ScanDir $AppDir -Mingw64Bin $Mingw64Bin -Msys2UsrBin $Msys2UsrBin
foreach ($dll in ($needed2 | Sort-Object)) {
    $src = Join-Path $Mingw64Bin $dll
    if (Test-Path $src) {
        try {
            Copy-Item $src (Join-Path $AppDir $dll) -Force
            Write-Host "  Copied (pass 2): $dll" -ForegroundColor Gray
        } catch {
            Write-Host "  Skipped (in use): $dll" -ForegroundColor Yellow
        }
    }
}
Write-Host "  Done. Total DLLs in app: $((Get-ChildItem $AppDir -Filter '*.dll').Count)" -ForegroundColor Green

# ── Step 7: Copy ffmpeg and its dependencies ─────────────────────────────────

Write-Step "Copying ffmpeg.exe and codec dependencies"

$ffmpegSrc = Join-Path $Mingw64Bin "ffmpeg.exe"
if (Test-Path $ffmpegSrc) {
    try {
        Copy-Item $ffmpegSrc (Join-Path $AppDir "ffmpeg.exe") -Force
        Write-Host "  Copied: ffmpeg.exe" -ForegroundColor Gray
    } catch {
        Write-Host "  Skipped (in use): ffmpeg.exe" -ForegroundColor Yellow
    }

    # Discover and copy ffmpeg codec DLLs (av*, sw*, etc.)
    $ldd = Join-Path $Mingw64Bin "ldd.exe"
    if (-not (Test-Path $ldd)) { $ldd = Join-Path $Msys2UsrBin "ldd.exe" }
    $ffmpegDeps = & $ldd (Join-Path $AppDir "ffmpeg.exe") 2>$null |
        Where-Object { $_ -match "^\s+(\S+\.dll)\s+=>\s+/mingw64/bin/" } |
        ForEach-Object { $_ -replace ".*?(\S+\.dll)\s+=>\s+.*", '$1' }
    foreach ($dll in $ffmpegDeps) {
        $src = Join-Path $Mingw64Bin $dll
        $dst = Join-Path $AppDir $dll
        if ((Test-Path $src) -and -not (Test-Path $dst)) {
            try {
                Copy-Item $src $dst -Force
                Write-Host "  Copied (ffmpeg dep): $dll" -ForegroundColor Gray
            } catch {
                Write-Host "  Skipped (in use): $dll" -ForegroundColor Yellow
            }
        }
    }
} else {
    Write-Host "  Warning: ffmpeg.exe not found in $Mingw64Bin - video export will not work" -ForegroundColor Yellow
}

# ── Step 8: Package as portable ZIP ──────────────────────────────────────────

Write-Step "Creating portable ZIP"

$ZipName = "virtualbow-portable-$AppVersion-windows-x64.zip"
$ZipPath = Join-Path $OutputDir $ZipName

if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }

# Stage a "virtualbow/" root with a "lib/" subfolder that holds every binary
# (exe, DLLs, Qt plugins, user-manual) and a tiny launcher at the top.
# Windows resolves DLL imports relative to the exe's own directory and Qt
# discovers its plugin folders (platforms/, imageformats/, ...) the same way,
# so the layout stays truly portable: no PATH edits, no qt.conf, no installer.
$StageRoot = Join-Path $BuildDir "stage"
$StageApp  = Join-Path $StageRoot "virtualbow"
$StageLib  = Join-Path $StageApp  "lib"
if (Test-Path $StageRoot) { Remove-Item $StageRoot -Recurse -Force }
New-Item -ItemType Directory -Path $StageLib -Force | Out-Null

Copy-Item -Path (Join-Path $AppDir '*') -Destination $StageLib -Recurse -Force

# Documentation & examples bundled at the top level so users can find them
# without digging into lib/. The user-manual stays inside lib/ as well, since
# the GUI's Help menu opens it via a path relative to the executable.
$DocsSrc      = Join-Path $ScriptDir "docs"
$ExamplesSrc  = Join-Path $DocsSrc   "examples"
$ExamplesDst  = Join-Path $StageApp  "examples"
if (Test-Path $ExamplesSrc) {
    New-Item -ItemType Directory -Path $ExamplesDst -Force | Out-Null
    # Skip VCS metadata and editor backup files; ship only the curated content.
    Copy-Item -Path (Join-Path $ExamplesSrc '*') -Destination $ExamplesDst -Recurse -Force `
        -Exclude '.gitignore','*.bak'
    # Copy-Item -Exclude only filters the top level; prune any leftovers deeper in.
    Get-ChildItem $ExamplesDst -Recurse -Force -Include '.gitignore','*.bak' |
        Remove-Item -Force -ErrorAction SilentlyContinue
    Write-Host "  Bundled examples/ ($((Get-ChildItem $ExamplesDst -Recurse -File).Count) files)" -ForegroundColor Gray
} else {
    Write-Host "  Warning: $ExamplesSrc not found; examples not bundled" -ForegroundColor Yellow
}

# Top-level shortcut to the user manual that already lives inside lib/.
# Use a tiny .cmd launcher (relative path, %~dp0) instead of duplicating
# ~30 MB of HTML or shipping a .lnk (which would bake in an absolute path).
$UserManualLib = Join-Path $StageLib "user-manual"
if (Test-Path $UserManualLib) {
    $manualLauncher = @'
@echo off
rem Opens the bundled user manual in the system default browser.
start "" "%~dp0lib\user-manual\index.html"
'@
    Set-Content -Path (Join-Path $StageApp "User Manual.cmd") -Value $manualLauncher -Encoding ASCII
    Write-Host "  Created top-level 'User Manual.cmd' shortcut" -ForegroundColor Gray
}

# Theory manual: ship the LaTeX sources + any pre-built PDF that happens to
# be present. Building the PDF requires a TeX toolchain that we do not assume.
$TheorySrc = Join-Path $DocsSrc "theory-manual"
if (Test-Path $TheorySrc) {
    $TheoryPdf = Join-Path $TheorySrc "document.pdf"
    if (Test-Path $TheoryPdf) {
        Copy-Item $TheoryPdf (Join-Path $StageApp "theory-manual.pdf") -Force
        Write-Host "  Bundled theory-manual.pdf" -ForegroundColor Gray
    } else {
        Write-Host "  Skipped theory-manual: no pre-built PDF at $TheoryPdf" -ForegroundColor DarkGray
    }
}

# Launcher: starts virtualbow-gui.exe from inside lib/ so its directory is
# searched for DLLs and Qt plugins automatically. /B keeps the console hidden.
$LauncherPath = Join-Path $StageApp "VirtualBow.cmd"
$launcher = @'
@echo off
rem VirtualBow portable launcher
rem Starts the GUI from the lib/ subdirectory so Windows resolves DLLs and
rem Qt plugins relative to the executable's own location. Forward any args.
start "" /D "%~dp0lib" "%~dp0lib\virtualbow-gui.exe" %*
'@
Set-Content -Path $LauncherPath -Value $launcher -Encoding ASCII

Compress-Archive -Path (Join-Path $StageRoot '*') -DestinationPath $ZipPath -CompressionLevel Optimal

$zipSize = [math]::Round((Get-Item $ZipPath).Length / 1MB, 1)
Write-Host "`nPortable package created: $ZipPath ($zipSize MB)" -ForegroundColor Green
$topItems = Get-ChildItem $StageApp |
    Select-Object Name, @{N="Size(KB)";E={[math]::Round($_.Length/1KB,0)}} |
    Format-Table -AutoSize | Out-String
Write-Host "ZIP root layout (virtualbow/):" -ForegroundColor Cyan
Write-Host $topItems
