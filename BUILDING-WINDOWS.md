# Building VirtualBow on Windows (MSYS2 / MinGW-w64)

This document covers a **developer build** of the VirtualBow GUI on Windows
using MSYS2's mingw64 toolchain plus CMake + Ninja. For producing a
redistributable installer, see [build-portable.ps1](build-portable.ps1).

## Prerequisites

1. **MSYS2** installed anywhere — https://www.msys2.org/
   The build script auto-detects common locations (`C:\msys64`,
   `D:\msys64`, `%USERPROFILE%\msys64`, `%LOCALAPPDATA%\msys64`,
   `%ProgramFiles%\msys64`). Override with `-Msys2Root <path>` or
   `$env:MSYS2_ROOT`.
2. **Rust** (stable, from https://rustup.rs/) on `PATH`.

That's it. The script installs every other prerequisite (mingw64 GCC,
CMake, Ninja, Qt6, nlohmann-json, Catch2, the `x86_64-pc-windows-gnu` Rust
target) automatically.

You do **not** need Visual Studio, Qt Creator, or any other MinGW
distribution. In fact, having a stray `C:\MinGW\bin` ahead of MSYS2 on
`PATH` will break the build silently — see *Troubleshooting* below.

## Quick start

From the repository root in PowerShell:

```powershell
.\build-dev.ps1                          # build virtualbow-gui (default)
.\build-dev.ps1 -Target virtualbow-test  # build the unit-test target
.\build-dev.ps1 -Reconfigure             # re-run cmake before building
.\build-dev.ps1 -Clean                   # wipe build/ and start fresh
.\build-dev.ps1 -Msys2Root D:\msys64     # override MSYS2 location
.\build-dev.ps1 -NoInstall               # skip prereq verification
```

The script:

1. Auto-detects the MSYS2 install (or honours `-Msys2Root` /
   `$env:MSYS2_ROOT`) and verifies `mingw64\bin\gcc.exe` is present.
2. Prepends `%SystemRoot%\System32;%SystemRoot%;<msys2>\mingw64\bin;<msys2>\usr\bin`
   to `PATH` (in that order — see notes).
3. Installs any missing mingw64 packages via `pacman -S --needed`.
4. Adds the `x86_64-pc-windows-gnu` Rust target if not already installed.
5. Builds the Rust FFI static library with `cargo build --release`.
6. Runs `cmake -G Ninja -DCMAKE_BUILD_TYPE=Release ..\gui` if `build/`
   doesn't already have a configured `build.ninja`.
7. Runs `ninja <target>`.

Output goes to `build\application\virtualbow-gui.exe`.

## Manual build (without the script)

Replace `<msys2>` with your MSYS2 install root:

```powershell
$msys2 = "C:\msys64"   # or wherever you installed it
$env:PATH = "$env:SystemRoot\System32;$env:SystemRoot;$msys2\mingw64\bin;$msys2\usr\bin;" + $env:PATH

mkdir build -ErrorAction SilentlyContinue
cd build
cmake -G Ninja -DCMAKE_BUILD_TYPE=Release ..\gui
ninja virtualbow-gui
```

## Why the PATH ordering matters

The order in `build-dev.ps1` is deliberate:

| Position | Entry                       | Reason                                                     |
| -------- | --------------------------- | ---------------------------------------------------------- |
| 1        | `%SystemRoot%\System32`     | Ensures `cmd.exe` resolves to the real Windows shell, not the MSYS one (which can't run programs in the middle of a pipeline). |
| 2        | `%SystemRoot%`              | Same.                                                      |
| 3        | `<msys2>\mingw64\bin`       | gcc, g++, ninja, moc, qmake, and all Qt6 + GCC runtime DLLs. |
| 4        | `<msys2>\usr\bin`           | sh, sed, etc. for any shell escapes in CMake-generated rules. |
| 5+       | inherited PATH              | Anything else.                                             |

Putting `mingw64\bin` *after* `System32` but *before* whatever the user has
in their environment guarantees: (a) the compilers can find their own DLLs,
and (b) a competing `C:\MinGW\bin` (or Strawberry Perl, or Git's bundled
gcc, or Chocolatey's) cannot be picked up first.
## Project structure (build inputs)

- [gui/CMakeLists.txt](gui/CMakeLists.txt) — top-level CMake project for the
  GUI executable. Pulls in the Rust solver via `virtualbow_ffi` (built as a
  static library by Cargo).
- [gui/source/](gui/source/) — C++ Qt6 sources.
- [rust/](rust/) — Rust workspace; the FFI crate is invoked from CMake.

## Troubleshooting

### `Test run of moc executable failed` / silent `exit 1` from ninja

**Symptom.** CMake configure fails on Qt6's `find_package` with a vague
"Test run of moc executable failed", or ninja prints nothing but exits 1.

**Cause.** A required runtime DLL cannot be loaded by `moc.exe`,
`cc1plus.exe`, or `ninja.exe`. Windows aborts the process with
`STATUS_DLL_NOT_FOUND` (`0xc0000135`) before any output is written.

The most common reason is `<msys2>\mingw64\bin` not being on `PATH`.
Another is `C:\MinGW\bin` (an old MinGW.org install) appearing first and
shadowing the MSYS2 toolchain.

**Fix.** Use `build-dev.ps1`, or set `PATH` manually as shown above.

### `'cmd.exe' is not recognized` / "Cannot run a document in the middle of a pipeline"

You shadowed Windows' `cmd.exe` with MSYS2's. Make sure
`%SystemRoot%\System32` comes **before** `<msys2>\usr\bin` on `PATH`.

### `QAbstractItemModelTester: No such file or directory`

The header is part of the `Qt6::Test` component. The GUI does not link
against `Qt6::Test`, so this include should not be present in any
non-test source. Remove the `#include` (it's a debug leftover).

### Path contains parentheses

Some Qt-generated defines (e.g. `QT_TESTCASE_BUILDDIR`) embed the build
path literally. If your home folder has `(` or `)` in its name and you
link against `Qt6::Test`, the unquoted `(` can break MOC-generated code.
The GUI target intentionally does **not** link `Qt6::Test` to avoid this;
keep it that way.

## Related

- [build-portable.ps1](build-portable.ps1) — produces a self-contained
  release ZIP / installer.
- [build-dev.ps1](build-dev.ps1) — the script described above.
