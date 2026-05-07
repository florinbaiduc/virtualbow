# Building VirtualBow on Windows (MSYS2 / MinGW-w64)

This document covers a **developer build** of the VirtualBow GUI on Windows
using MSYS2's mingw64 toolchain plus CMake + Ninja. For producing a
redistributable installer, see [build-portable.ps1](build-portable.ps1).

## Prerequisites

1. **MSYS2** installed at `C:\msys64` — https://www.msys2.org/
2. From an MSYS2 mingw64 shell, install the toolchain and Qt6:
   ```bash
   pacman -S --needed \
       mingw-w64-x86_64-gcc \
       mingw-w64-x86_64-cmake \
       mingw-w64-x86_64-ninja \
       mingw-w64-x86_64-qt6-base \
       mingw-w64-x86_64-qt6-tools
   ```
3. **Rust** (stable) with the `x86_64-pc-windows-gnu` target:
   ```powershell
   rustup target add x86_64-pc-windows-gnu
   ```

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
```

The script:

1. Sanity-checks that `C:\msys64\mingw64\bin` exists.
2. Prepends `C:\Windows\System32;C:\Windows;C:\msys64\mingw64\bin;C:\msys64\usr\bin`
   to `PATH` (in that order — see notes).
3. Runs `cmake -G Ninja -DCMAKE_BUILD_TYPE=Release ..\gui` if `build/`
   doesn't already have a configured `build.ninja`.
4. Runs `ninja <target>`.

Output goes to `build\application\virtualbow-gui.exe`.

## Manual build (without the script)

```powershell
# Make sure mingw64/bin comes before any other gcc/Qt on PATH.
$env:PATH = "C:\Windows\System32;C:\Windows;C:\msys64\mingw64\bin;C:\msys64\usr\bin;" + $env:PATH

mkdir build -ErrorAction SilentlyContinue
cd build
cmake -G Ninja -DCMAKE_BUILD_TYPE=Release ..\gui
ninja virtualbow-gui
```

## Why the PATH ordering matters

The order in `build-dev.ps1` is deliberate:

| Position | Entry                     | Reason                                                     |
| -------- | ------------------------- | ---------------------------------------------------------- |
| 1        | `C:\Windows\System32`     | Ensures `cmd.exe` resolves to the real Windows shell, not the MSYS one (which can't run programs in the middle of a pipeline). |
| 2        | `C:\Windows`              | Same.                                                      |
| 3        | `C:\msys64\mingw64\bin`   | gcc, g++, ninja, moc, qmake, and all Qt6 + GCC runtime DLLs. |
| 4        | `C:\msys64\usr\bin`       | sh, sed, etc. for any shell escapes in CMake-generated rules. |
| 5+       | inherited PATH            | Anything else.                                             |

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

The most common reason is `C:\msys64\mingw64\bin` not being on `PATH`.
Another is `C:\MinGW\bin` (an old MinGW.org install) appearing first and
shadowing the MSYS2 toolchain.

**Fix.** Use `build-dev.ps1`, or set `PATH` manually as shown above.

### `'cmd.exe' is not recognized` / "Cannot run a document in the middle of a pipeline"

You shadowed Windows' `cmd.exe` with MSYS2's. Make sure
`C:\Windows\System32` comes **before** `C:\msys64\usr\bin` on `PATH`.

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
