// Bow file version 5
// Used in VirtualBow input 0.11.*
//
// Major change vs v4: full asymmetric bow support.
// - `Profile` and `Section` are split into independent `upper` and `lower` halves.
// - Tip masses are split into upper / lower fields and the legacy `string_center`
//   becomes `string_nock`.
// - `RigidHandle.length` is split into `length_upper` / `length_lower`.
// - `Draw` gains `nock_offset` so the nocking point can sit off the geometric
//   center of the bow (the defining feature of a yumi).

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use super::{version1, version4};

pub use version1::Width;
pub use version1::Height;
pub use version1::BowString;
pub use version1::Damping;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BowModel {
    pub comment: String,
    pub settings: Settings,
    pub handle: Handle,
    pub draw: Draw,
    pub profile: Profile,
    pub section: Section,
    pub string: BowString,
    pub masses: Masses,
    pub damping: Damping,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Settings {
    pub num_limb_elements: usize,
    pub num_limb_eval_points: usize,
    pub min_draw_resolution: usize,
    pub max_draw_resolution: usize,
    pub static_iteration_tolerance: f64,
    pub arrow_clamp_force: f64,
    pub string_compression_factor: f64,
    pub timespan_factor: f64,
    pub timeout_factor: f64,
    pub min_timestep: f64,
    pub max_timestep: f64,
    pub steps_per_period: usize,
    pub dynamic_iteration_tolerance: f64
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            num_limb_elements: 30,
            num_limb_eval_points: 250,
            min_draw_resolution: 100,
            max_draw_resolution: 100,
            static_iteration_tolerance: 1e-6,
            arrow_clamp_force: 0.5,
            string_compression_factor: 1e-6,
            timespan_factor: 1.5,
            timeout_factor: 10.0,
            min_timestep: 1e-6,
            max_timestep: 1e-4,
            steps_per_period: 250,
            dynamic_iteration_tolerance: 1e-6
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Draw {
    pub brace_height: f64,
    pub draw_length: DrawLength,
    /// Signed offset of the nocking point along the bow's longitudinal axis,
    /// measured from the handle pivot point (positive = toward upper limb).
    /// Zero for a symmetric bow. For a yumi, typically ≈ +(L/6).
    #[serde(default)]
    pub nock_offset: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DrawLength {
    Standard(f64),    // Measured from the handle's belly or pivot point
    Amo(f64)          // Same but with 1.75" offset according to the AMO standard
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Handle {
    Flexible,
    Rigid(RigidHandle),
    /// A flexible grip that extends through the joint between the upper and
    /// lower limbs, modelled as two beam cantilevers (one per side) sharing
    /// a clamped pivot at the geometric center. Recovers grip-region bending
    /// stiffness and strain energy that the rigid handle discards. Designed
    /// for continuous-limb bows such as the yumi where the limbs are made
    /// from a single nock-to-nock laminate.
    Beam(BeamHandle),
}

// Parameters for a rigid handle section between the bow limbs.
// `length_upper` and `length_lower` are the two halves of the rigid grip,
// extending from the pivot toward the upper and lower limbs respectively.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RigidHandle {
    pub length_upper: f64,
    pub length_lower: f64,
    pub angle: f64,
    pub pivot: f64
}

impl RigidHandle {
    /// Total handle length (used in geometry calculations as before).
    pub fn total_length(&self) -> f64 {
        self.length_upper + self.length_lower
    }

    /// Signed offset of the geometric handle center from the pivot point.
    /// Positive = handle's mid-point sits above the pivot (toward the upper limb).
    pub fn center_offset(&self) -> f64 {
        0.5 * (self.length_upper - self.length_lower)
    }
}

/// Parameters for a flexible beam handle. Geometrically identical to
/// `RigidHandle` (`length_upper`, `length_lower`, `angle`, `pivot`) but
/// adds a grip cross-section (`section`) that is meshed into FEM beam
/// elements during simulation. The pivot end of each grip half is
/// fully clamped; the limb-side end coincides with the corresponding
/// limb's root node so the laminate is structurally continuous through
/// the joint.
///
/// Mechanically equivalent to extending each limb's profile by a Line
/// of the corresponding length and using the grip section there, with
/// the advantage that the grip cross-section is specified once instead
/// of being woven into the limb section splines.
///
/// Note: at present the two grip halves are independently clamped at
/// the pivot, so this variant recovers grip-region flexure (I1/I2/I5
/// in the design notes) but does not model handle rotational compliance
/// (I3) — that requires a rigid-link / shared-rotational-DOF mechanism
/// not yet present in the FEM core.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BeamHandle {
    pub length_upper: f64,
    pub length_lower: f64,
    pub angle: f64,
    pub pivot: f64,
    /// Number of beam elements meshing the upper grip half.
    #[serde(default = "BeamHandle::default_n_elements")]
    pub n_elements_upper: usize,
    /// Number of beam elements meshing the lower grip half.
    #[serde(default = "BeamHandle::default_n_elements")]
    pub n_elements_lower: usize,
    /// Cross-section of the grip (the laminate continuing through the joint).
    pub section: LimbSection,
}

impl BeamHandle {
    fn default_n_elements() -> usize { 4 }

    pub fn total_length(&self) -> f64 {
        self.length_upper + self.length_lower
    }

    pub fn center_offset(&self) -> f64 {
        0.5 * (self.length_upper - self.length_lower)
    }
}

/// A single limb's cross-section (independent for upper and lower limbs).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LimbSection {
    pub alignment: LayerAlignment,
    pub width: Width,
    pub layers: Vec<Layer>,
}

/// Bow-wide section data: upper & lower limb sections plus the shared materials list.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Section {
    pub materials: Vec<Material>,
    pub upper: LimbSection,
    pub lower: LimbSection,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub color: String,
    pub density: f64,
    pub youngs_modulus: f64,
    pub shear_modulus: f64,
    pub tensile_strength: f64,
    pub compressive_strength: f64,
    pub safety_margin: f64
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    pub material: String,
    pub height: Height,
}

/// Full-bow profile: independent upper & lower limb segment lists.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Profile {
    pub upper: Vec<ProfileSegment>,
    pub lower: Vec<ProfileSegment>,
}

// Defines how the cross-sections are aligned with the profile curve
// There are two categories:
// - Section: The profile curve is aligned with the back side, belly side, or geometrical center of the combined section
// - Layer: The profile curve is aligned with the back side, belly side, or geometrical center of a specific layer
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "layer", rename_all = "snake_case")]
pub enum LayerAlignment {
    SectionBack,
    SectionBelly,
    SectionCenter,
    LayerBack(String),
    LayerBelly(String),
    LayerCenter(String)
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "parameters", rename_all = "snake_case")]
pub enum ProfileSegment {
    Line(Line),
    Arc(Arc),
    Spiral(Spiral),
    Spline(Spline),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Line {
    pub length: f64
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Arc {
    pub length: f64,
    pub radius: f64
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Spiral {
    pub length: f64,
    pub radius_start: f64,
    pub radius_end: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Spline {
    pub points: Vec<[f64; 2]>
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ArrowMass {
    Mass(f64),
    MassPerForce(f64),
    MassPerEnergy(f64)
}

/// All point-mass contributions on the bow.
/// Tip masses are independent for upper and lower limbs / string ends.
/// `string_nock` replaces v4's `string_center` (mass at the arrow nocking point).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Masses {
    pub arrow: ArrowMass,
    pub limb_tip_upper: f64,
    pub limb_tip_lower: f64,
    pub string_nock: f64,
    pub string_tip_upper: f64,
    pub string_tip_lower: f64,
}

/// Identifier for one of the two limbs.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LimbSide {
    Upper,
    Lower,
}

impl LimbSide {
    pub const ALL: [LimbSide; 2] = [LimbSide::Upper, LimbSide::Lower];

    /// +1 for the upper limb (extending in +y from the grip),
    /// -1 for the lower limb (extending in -y).
    pub fn sign(self) -> f64 {
        match self {
            LimbSide::Upper => 1.0,
            LimbSide::Lower => -1.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LimbSide::Upper => "upper",
            LimbSide::Lower => "lower",
        }
    }
}

// =====================================================================
// Migration: v4 (symmetric, half-bow) -> v5 (asymmetric, full-bow)
//
// Existing v4 data describes a single half (grip-to-tip). Mapping to v5:
//   - Profile / Section are duplicated into both `upper` and `lower`.
//   - RigidHandle.length is split in half (length_upper = length_lower = length/2).
//   - Tip masses are duplicated; v4.string_center -> v5.string_nock.
//   - Draw.nock_offset = 0 (symmetric).
// =====================================================================
impl From<version4::BowModel> for BowModel {
    fn from(model: version4::BowModel) -> BowModel {
        let v4_profile_segments = model.profile.segments.iter().map(convert_v4_segment).collect_vec();

        let upper_section = LimbSection {
            alignment: convert_v4_alignment(&model.section.alignment),
            width: model.section.width.clone(),
            layers: model.section.layers.iter().map(convert_v4_layer).collect_vec(),
        };
        let lower_section = upper_section.clone();

        let materials = model.section.materials.iter().map(convert_v4_material).collect_vec();
        let section = Section {
            materials,
            upper: upper_section,
            lower: lower_section,
        };

        let profile = Profile {
            upper: v4_profile_segments.clone(),
            lower: v4_profile_segments,
        };

        let handle = match model.handle {
            version4::Handle::Flexible => Handle::Flexible,
            version4::Handle::Rigid(h) => Handle::Rigid(RigidHandle {
                length_upper: 0.5 * h.length,
                length_lower: 0.5 * h.length,
                angle: h.angle,
                pivot: h.pivot,
            }),
        };

        let draw = Draw {
            brace_height: model.draw.brace_height,
            draw_length: match model.draw.draw_length {
                version4::DrawLength::Standard(v) => DrawLength::Standard(v),
                version4::DrawLength::Amo(v) => DrawLength::Amo(v),
            },
            nock_offset: 0.0,
        };

        let masses = Masses {
            arrow: match model.masses.arrow {
                version4::ArrowMass::Mass(v) => ArrowMass::Mass(v),
                version4::ArrowMass::MassPerForce(v) => ArrowMass::MassPerForce(v),
                version4::ArrowMass::MassPerEnergy(v) => ArrowMass::MassPerEnergy(v),
            },
            limb_tip_upper: model.masses.limb_tip,
            limb_tip_lower: model.masses.limb_tip,
            string_nock: model.masses.string_center,
            string_tip_upper: model.masses.string_tip,
            string_tip_lower: model.masses.string_tip,
        };

        let settings = Settings {
            num_limb_elements: model.settings.num_limb_elements,
            num_limb_eval_points: model.settings.num_limb_eval_points,
            min_draw_resolution: model.settings.min_draw_resolution,
            max_draw_resolution: model.settings.max_draw_resolution,
            static_iteration_tolerance: model.settings.static_iteration_tolerance,
            arrow_clamp_force: model.settings.arrow_clamp_force,
            string_compression_factor: model.settings.string_compression_factor,
            timespan_factor: model.settings.timespan_factor,
            timeout_factor: model.settings.timeout_factor,
            min_timestep: model.settings.min_timestep,
            max_timestep: model.settings.max_timestep,
            steps_per_period: model.settings.steps_per_period,
            dynamic_iteration_tolerance: model.settings.dynamic_iteration_tolerance,
        };

        Self {
            comment: model.comment,
            settings,
            handle,
            draw,
            profile,
            section,
            string: model.string,
            masses,
            damping: model.damping,
        }
    }
}

fn convert_v4_segment(s: &version4::ProfileSegment) -> ProfileSegment {
    match s {
        version4::ProfileSegment::Line(l) => ProfileSegment::Line(Line { length: l.length }),
        version4::ProfileSegment::Arc(a) => ProfileSegment::Arc(Arc { length: a.length, radius: a.radius }),
        version4::ProfileSegment::Spiral(s) => ProfileSegment::Spiral(Spiral {
            length: s.length,
            radius_start: s.radius_start,
            radius_end: s.radius_end,
        }),
        version4::ProfileSegment::Spline(s) => ProfileSegment::Spline(Spline { points: s.points.clone() }),
    }
}

fn convert_v4_alignment(a: &version4::LayerAlignment) -> LayerAlignment {
    match a {
        version4::LayerAlignment::SectionBack => LayerAlignment::SectionBack,
        version4::LayerAlignment::SectionBelly => LayerAlignment::SectionBelly,
        version4::LayerAlignment::SectionCenter => LayerAlignment::SectionCenter,
        version4::LayerAlignment::LayerBack(n) => LayerAlignment::LayerBack(n.clone()),
        version4::LayerAlignment::LayerBelly(n) => LayerAlignment::LayerBelly(n.clone()),
        version4::LayerAlignment::LayerCenter(n) => LayerAlignment::LayerCenter(n.clone()),
    }
}

fn convert_v4_material(m: &version4::Material) -> Material {
    Material {
        name: m.name.clone(),
        color: m.color.clone(),
        density: m.density,
        youngs_modulus: m.youngs_modulus,
        shear_modulus: m.shear_modulus,
        tensile_strength: m.tensile_strength,
        compressive_strength: m.compressive_strength,
        safety_margin: m.safety_margin,
    }
}

fn convert_v4_layer(l: &version4::Layer) -> Layer {
    Layer {
        name: l.name.clone(),
        material: l.material.clone(),
        height: l.height.clone(),
    }
}
