// Legacy symmetric bow schema (original upstream VirtualBow 0.10.*)
//
// This is NOT the fork's `version4` schema. Upstream VirtualBow shipped a
// single-limb ("symmetric") model under the same `version = "0.10.0"` tag,
// laid out very differently from this fork's v4:
//
//   - a `dimensions` block (handle_reference / handle_angle / handle_length /
//     handle_offset / brace_height / draw_length) instead of `handle` + `draw`,
//   - a single `section` (alignment / width / materials / layers) and a single
//     `profile.segments` list describing one limb half,
//   - single-limb point masses (`limb_tip`, `string_center`, `string_tip`),
//   - no `static_iteration_tolerance` / `dynamic_iteration_tolerance` settings.
//
// Files in this layout (e.g. docs/examples/bows/recurve.bow) cannot be parsed
// by the tagged `BowModelVersion` enum because the "0.10.0" tag routes them to
// the fork's `version4::BowModel`, whose fields don't match. `BowModelVersion::
// load` therefore falls back to this module when a "0.10.0" file carries a
// `dimensions` block, and converts it straight to the latest schema by
// mirroring the single limb into both the upper and lower halves.

use serde::{Deserialize, Serialize};

use super::latest;
use super::version1;

/// Default iteration tolerance for settings fields absent from the upstream
/// 0.10 schema (matches `latest::Settings::default()`).
fn default_iteration_tolerance() -> f64 {
    1e-6
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BowModel {
    pub comment: String,
    pub settings: Settings,
    pub dimensions: Dimensions,
    pub profile: Profile,
    pub section: Section,
    pub string: latest::BowString,
    pub masses: version1::Masses,
    pub damping: latest::Damping,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Settings {
    pub num_limb_elements: usize,
    pub num_limb_eval_points: usize,
    pub min_draw_resolution: usize,
    pub max_draw_resolution: usize,
    #[serde(default = "default_iteration_tolerance")]
    pub static_iteration_tolerance: f64,
    pub arrow_clamp_force: f64,
    pub string_compression_factor: f64,
    pub timespan_factor: f64,
    pub timeout_factor: f64,
    pub min_timestep: f64,
    pub max_timestep: f64,
    pub steps_per_period: usize,
    #[serde(default = "default_iteration_tolerance")]
    pub dynamic_iteration_tolerance: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Dimensions {
    pub handle_reference: String,
    pub handle_angle: f64,
    pub handle_length: f64,
    pub handle_offset: f64,
    pub brace_height: f64,
    pub draw_length: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Profile {
    pub segments: Vec<latest::ProfileSegment>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Section {
    pub alignment: latest::LayerAlignment,
    pub width: latest::Width,
    pub materials: Vec<latest::Material>,
    pub layers: Vec<latest::Layer>,
}

// =====================================================================
// Migration: upstream symmetric 0.10 (single half-bow) -> latest v5
// (asymmetric full-bow). The single limb is mirrored into both the upper
// and lower halves and the symmetry flags are set so the GUI presents the
// bow as symmetric.
// =====================================================================
impl From<BowModel> for latest::BowModel {
    fn from(model: BowModel) -> latest::BowModel {
        let BowModel { comment, settings, dimensions, profile, section, string, masses, damping } = model;

        let limb_section = latest::LimbSection {
            alignment: section.alignment,
            width: section.width,
            layers: section.layers,
        };

        latest::BowModel {
            comment,
            settings: latest::Settings {
                num_limb_elements: settings.num_limb_elements,
                num_limb_eval_points: settings.num_limb_eval_points,
                min_draw_resolution: settings.min_draw_resolution,
                max_draw_resolution: settings.max_draw_resolution,
                static_iteration_tolerance: settings.static_iteration_tolerance,
                arrow_clamp_force: settings.arrow_clamp_force,
                string_compression_factor: settings.string_compression_factor,
                timespan_factor: settings.timespan_factor,
                timeout_factor: settings.timeout_factor,
                min_timestep: settings.min_timestep,
                max_timestep: settings.max_timestep,
                steps_per_period: settings.steps_per_period,
                dynamic_iteration_tolerance: settings.dynamic_iteration_tolerance,
            },
            handle: latest::Handle::Rigid(latest::RigidHandle {
                length_upper: 0.5 * dimensions.handle_length,
                length_lower: 0.5 * dimensions.handle_length,
                angle: dimensions.handle_angle,
                pivot: dimensions.handle_offset,
            }),
            draw: latest::Draw {
                brace_height: dimensions.brace_height,
                draw_length: latest::DrawLength::Standard(dimensions.draw_length),
                nock_offset: 0.0,
            },
            profile: latest::Profile {
                upper: profile.segments.clone(),
                lower: profile.segments,
            },
            section: latest::Section {
                materials: section.materials,
                upper: limb_section.clone(),
                lower: limb_section,
            },
            string,
            masses: latest::Masses {
                arrow: latest::ArrowMass::Mass(masses.arrow),
                limb_tip_upper: masses.limb_tip,
                limb_tip_lower: masses.limb_tip,
                string_nock: masses.string_center,
                string_tip_upper: masses.string_tip,
                string_tip_lower: masses.string_tip,
            },
            damping,
            symmetry: latest::Symmetry {
                profile: true,
                width: true,
                layers: true,
            },
        }
    }
}
