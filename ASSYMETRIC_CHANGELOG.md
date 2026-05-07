# Changes Since Yesterday

Period: April 27–28, 2026. All changes are uncommitted (working tree).

The work falls into four threads, in chronological order:

1. **Yumi accuracy investigation and countermeasures** (Phase A demonstrator + Phase B1 FEM grip + Phase B3 continuity warnings).
2. **Grip rendering bug-hunt** (2D `ShapePlot`, 3D `LimbView`, multiple iterations).
3. **Conservative Rust-core dead-code cleanup**.
4. **"Save as video" export** for the dynamic Shape window.

---

## 1. Yumi accuracy work

### Background
A yumi has continuous limbs from nock to nock; VirtualBow's bow model assumes two independent cantilever limbs clamped to a rigid massless handle. We identified seven inaccuracies (I1–I7) and proposed a tiered plan (Phase A no-code, Phase B light extensions, Phase C deferred deep refactors).

### Phase A — flexible-handle demonstrator (no code)
Added [`docs/examples/bows/yumi_phaseA.bow`](docs/examples/bows/yumi_phaseA.bow): a `Handle::Flexible` variant of the yumi with the rigid grip portion folded into the limb's profile (extra `Line` segments) and matching joint section. Captures most of the grip flexure that the rigid-handle model misses, with no code change.

Baseline vs Phase A measurements: −5 % draw force, −10 % arrow KE.

### Phase B1 — `Handle::Beam` FEM variant
A new handle variant that meshes the grip itself with corotational beam elements clamped at the pivot (instead of clamping the limb root).

**`rust/virtualbow/src/input/versions/latest.rs`**
- Added `Handle::Beam(BeamHandle)` enum arm.
- New `BeamHandle { length_upper, length_lower, angle, pivot, n_elements_upper (default 4), n_elements_lower (default 4), section: LimbSection }`.

**`rust/virtualbow/src/input/model.rs`**
- Updated `Handle::validate()` for the Beam case.
- Changed `Handle::to_rigid()` to return owned `RigidHandle` (was `&RigidHandle`); synthesises an equivalent rigid handle from each variant for legacy code paths.
- Added `Handle::beam_section() -> Option<&LimbSection>`.

**`rust/virtualbow/src/simulation.rs`**
- New `GripEvalInfo` struct (length, width, height, bounds, ratio, heights, position_eval, position_control, curvature_eval).
- `LimbChain` gained `grip_elements: Vec<usize>` and `grip_eval: Option<GripEvalInfo>`.
- `build_chain` now takes `existing_root: Option<Node>` (uses the provided node as `nodes[0]` instead of creating a clamped one — used to chain a limb onto a grip's joint node).
- New `build_grip_chain(system, side, grip_length, section, materials, n_elements)` builds a Line-profile beam chain locked at the pivot end. The lower side is mirrored into world frame via `(x, y, φ) → (-x, y, π−φ)` and `T6·K·T6` similarity transforms.
- `eval_chain` iterates `chain.grip_elements.iter().chain(chain.elements.iter())` so grip data are produced first.
- `layer_results` zero-pads the grip portion of layer eval points.
- New `prepend_grip_to_limb_info` helper concatenates grip+limb arrays for `Common.limb` and `Common.limb_lower` so result viewers see one continuous outline through the grip.

**Demonstrator**: [`docs/examples/bows/yumi_phaseB1.bow`](docs/examples/bows/yumi_phaseB1.bow), bamboo / wood-core / bamboo grip section.

Measured deltas vs the rigid-handle baseline: −11.49 % draw force, −19.64 % limb energy, +8.87 % efficiency.

### Phase B3 — continuity warnings
**`rust/virtualbow/src/input/model.rs`**: new `BowModel::continuity_warnings() -> Vec<String>` checks alignment, width, layer count, material, and height continuity at the upper/lower limb joint.

**`rust/virtualbow_cli/src/main.rs`**: prints each warning to stderr (`WARNING: …`) after model load. Already surfaced a silent thickness mismatch (0.0109 m vs 0.011 m) in the existing [`yumi.bow`](docs/examples/bows/yumi.bow) and [`yumi_curved1.bow`](docs/examples/bows/yumi_curved1.bow).

### Verification
All 108 Rust tests still pass (25 unit + 8 example-bow + the rest). `yumi_phaseB1.bow` produces 259 limb_pos eval points (250 limb + 9 grip per side, n_elements=4 → 1+2·4 = 9), baseline `yumi.bow` unchanged at 250.

---

## 2. Grip rendering iterations

The simulation result viewer's 2D Shape plot ([`gui/source/post/ShapePlot.cpp`](gui/source/post/ShapePlot.cpp)) and the editor's 3D limb preview ([`gui/source/pre/views/limb3d/LimbView.cpp`](gui/source/pre/views/limb3d/LimbView.cpp)) needed coordinated updates so the grip would visibly join the limb outlines for all bow topologies, including yumi-style joints with near-opposite limb tangents.

### 2D `ShapePlot` — `plotHandle`
- **v0** (initial): black 2-point line. → "looks like a single line" for any bow.
- **v1**: filled quadrilateral built from each limb's local back/belly normal, depth scaled to 25 % of grip length. → produced a self-intersecting bow-tie polygon for yumi and any bow whose upper/lower inboard tangents are roughly opposite, because the lower-side normal flip puts the lower back/belly corners on the same side as the upper ones.
- **v2**: rectangle perpendicular to grip axis, depth scaled to 25 % of grip length. → fixed the bow-tie but the rectangle was disjoint from the limb outlines (visible black box floating between the limbs).
- **v3**: closed polygon whose four corners coincided with the inboard back/belly corners of the limb outlines, using the same `s = ±1` formula as `plotLimbOutline`. → re-introduced the bow-tie for yumi (same root cause as v1: per-limb normals flip in opposite directions).
- **v4 (current)**: rectangle perpendicular to grip axis with **half-depth = limb laminate thickness** (so the rectangle's long edges line up with the limb back/belly outlines for any bow), and **skip drawing entirely when `grip_length < 2 × half_depth`** — i.e. when the limbs already touch within their own thickness (continuous-joint / yumi-style bow), the limb outlines themselves close the joint and any extra polygon would just produce the bow-tie again.

Also added a `setBrush(QColor(40, 40, 40))` so the polygon is filled rather than just outlined, and set the pen to 1.5px (was 2.5px). Added `<algorithm>` and `<cmath>` includes.

### 3D `LimbView` — rigid-handle block
- v0: existing implementation, used the limb laminate thickness for the back/belly extent of the 8-corner block — collapsed to a hairline at bow scale.
- v1: inflated half-depth to ~25 % of grip length (12–40 mm clamp). → too fat for short rigid handles.
- v2 (current): reverted the inflation; the block again uses the limb's true inboard cross-section (laminate bounds and width). The user reported this looked correct.

Added `<algorithm>` and `<cmath>` includes.

(`LimbView.cpp` still only handles the `RigidHandle` variant in 3D; for `Handle::Beam` the 3D block falls through to `nullptr`. The Phase B1 work renders correctly in the 2D result viewer because of `prepend_grip_to_limb_info`; 3D-editor support for `BeamHandle` remains a future task.)

---

## 3. Rust-core dead-code cleanup (conservative)

After the user requested a "massive refactor" then narrowed the scope to dead-code removal in `rust/virtualbow` and `rust/virtualbow_num` only, an in-depth scan turned up essentially nothing genuinely dead. Two micro-cleanups applied:

**`rust/virtualbow/src/simulation.rs`**: removed the unused `LimbChain.side: LimbSide` field (and its `#[allow(dead_code)]` attribute). Was assigned in the single constructor, never read. Updated the constructor.

**`rust/virtualbow/src/sections/section.rs`**: removed the misleading `let _ = layer_map; // currently unused beyond validation` line — the variable is in fact used downstream by the `LayerAlignment::LayerBack/Belly/Center` arms; the suppression line was outright wrong and confusing.

A third candidate (`compatibility.rs` `env!("CARGO_PKG_VERSION")` substitution and the "unnecessary clone" in `BowModelVersion::save`) was deferred: the version substitution is blocked by an open serde issue (per the existing TODO), and the clone removal would require a `BowModelVersion<'a>` borrowed variant — out of conservative scope.

All 108 Rust tests still pass.

---

## 4. "Save as video" export

User-requested feature for the dynamic-simulation Shape window.

### New files
- [`gui/source/post/ShapeVideoExporter.hpp`](gui/source/post/ShapeVideoExporter.hpp) / [`.cpp`](gui/source/post/ShapeVideoExporter.cpp)

### Behaviour
- Exposed via a **"Save as video..." button** placed in a dedicated toolbar at the **top of the Shape tab** inside `DynamicOutputWidget` (initially placed next to the slider, but the user reported it wasn't visible enough; relocating it to the tab toolbar made it impossible to miss).
- **ffmpeg is a hard requirement.** When ffmpeg is not on `PATH`, the export shows a blocking dialog with platform-specific install commands (apt / dnf / pacman / zypper / Flatpak on Linux; Homebrew / MacPorts on macOS; winget / Chocolatey / Scoop on Windows) plus a **Recheck** button that re-tests `QStandardPaths::findExecutable("ffmpeg")` and an **Open ffmpeg.org** button that launches the download page via `QDesktopServices::openUrl`. Cancelled = export aborts.
- Once ffmpeg is available: file dialog offers MP4 (H.264 / yuv420p / `+faststart`), WebM (VP9, CRF 32), or animated GIF (palettegen). FPS prompt (default 30, range 1–120).
- Renders every dynamic state via `ShapePlot::setStateIndex(i) → replot(rpImmediateRefresh) → toPixmap(width, height)`, saving each as `frame_%06d.png` in a `QTemporaryDir`. Cancellable `QProgressDialog` during rendering; second indeterminate progress dialog during ffmpeg encoding.
- Width/height forced to even via `scale=trunc(iw/2)*2:trunc(ih/2)*2` for H.264/VP9 (codec requirement).
- ffmpeg failures surface a `QMessageBox` with the merged stdout+stderr in the detail box.
- Slider position is saved before export and restored afterwards so the UI doesn't visibly jump.

### Edited files
- [`gui/source/post/OutputWidget.cpp`](gui/source/post/OutputWidget.cpp): added the toolbar wrapper for the Shape tab, the click handler, and the slider-position bookkeeping. `DynamicOutputWidget` only — the static window is unaffected.
- [`gui/CMakeLists.txt`](gui/CMakeLists.txt): added the two new source files to the `virtualbow-gui` target.

---

## Files inventoried

Additions: `gui/source/post/ShapeVideoExporter.{hpp,cpp}`, `docs/examples/bows/yumi_phaseA.bow`, `docs/examples/bows/yumi_phaseB1.bow`.

Modifications related to the work above: `gui/CMakeLists.txt`, `gui/source/post/OutputWidget.cpp`, `gui/source/post/ShapePlot.{cpp,hpp}`, `gui/source/pre/views/limb3d/LimbView.cpp`, `rust/virtualbow/src/simulation.rs`, `rust/virtualbow/src/sections/section.rs`, `rust/virtualbow/src/input/versions/latest.rs`, `rust/virtualbow/src/input/model.rs`, `rust/virtualbow_cli/src/main.rs`.

Other modifications visible in `git status` (e.g. `rust/virtualbow/src/input/versions/version4.rs`, `rust/virtualbow/src/input/compatibility.rs`, `gui/source/solver/BowModel.{cpp,hpp}`, model-editor docs, various GUI views) are pre-existing uncommitted work from before this conversation and are not described above.

## Verification status

- `cargo test -p virtualbow -p virtualbow_num` (release): 108 passed, 0 failed.
- `cmake --build . --target virtualbow-gui`: clean (no warnings).
- ffmpeg is not installed on this dev machine, so the encoded-video path was exercised at compile-time only; the install-prompt dialog is the runtime entry point that takes over.
