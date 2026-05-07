use std::path::Path;
use itertools::Itertools;
use crate::errors::ModelError;
use crate::utils::validation::{FloatValidation, IntegerValidation, StringValidation};

use super::compatibility::BowModelVersion;
pub use super::versions::latest::*;  // Export latest version to the outside and implement below

impl BowModel {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ModelError> {
        let version = BowModelVersion::load(path)?;
        version.get_latest()
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), ModelError> {
        BowModelVersion::save(path, self)
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { comment: _, settings, handle, draw, section, profile, string, masses, damping } = self;

        settings.validate()?;
        handle.validate()?;
        draw.validate()?;
        profile.validate()?;
        section.validate()?;
        string.validate()?;
        masses.validate()?;
        damping.validate()?;

        Ok(())
    }

    /// Non-fatal continuity checks at the upper/lower limb joint.
    ///
    /// The two-limb model fully clamps the upper- and lower-limb roots to a
    /// rigid handle. The upper.s=0 and lower.s=0 cross-sections are physically
    /// the same point on a continuous-limb bow (e.g. a yumi); silent
    /// mismatches there create a fictitious geometric/material kink at the
    /// joint and a misleading stress-recovery boundary condition.
    /// Returns a list of human-readable warnings; empty means no mismatch.
    pub fn continuity_warnings(&self) -> Vec<String> {
        const TOL: f64 = 1e-9;
        let mut w: Vec<String> = Vec::new();
        let u = &self.section.upper;
        let l = &self.section.lower;

        // Alignment
        if u.alignment != l.alignment {
            w.push(format!(
                "section.upper.alignment ({:?}) differs from section.lower.alignment ({:?}) at the joint",
                u.alignment, l.alignment
            ));
        }

        // Width at s=0
        let uw0 = u.width.0.first().map(|p| p[1]);
        let lw0 = l.width.0.first().map(|p| p[1]);
        if let (Some(uw), Some(lw)) = (uw0, lw0) {
            if (uw - lw).abs() > TOL {
                w.push(format!(
                    "section width at the joint differs: upper={uw} m, lower={lw} m"
                ));
            }
        }

        // Layer count
        if u.layers.len() != l.layers.len() {
            w.push(format!(
                "section layer count differs at the joint: upper has {}, lower has {}",
                u.layers.len(), l.layers.len()
            ));
        }

        // Per-layer material and s=0 height
        for (i, (ul, ll)) in u.layers.iter().zip(l.layers.iter()).enumerate() {
            if ul.material != ll.material {
                w.push(format!(
                    "section layer {i} material differs at the joint: upper=\"{}\", lower=\"{}\"",
                    ul.material, ll.material
                ));
            }
            let uh0 = ul.height.0.first().map(|p| p[1]);
            let lh0 = ll.height.0.first().map(|p| p[1]);
            if let (Some(uh), Some(lh)) = (uh0, lh0) {
                if (uh - lh).abs() > TOL {
                    w.push(format!(
                        "section layer {i} (\"{}\" / \"{}\") height at the joint differs: upper={uh} m, lower={lh} m",
                        ul.name, ll.name
                    ));
                }
            }
        }

        w
    }

    // Create a simple but valid example bow
    pub fn example() -> Self {
        Self {
            comment: "".into(),
            settings: Settings::default(),
            handle: Handle::Flexible,
            draw: Draw {
                brace_height: 0.2,
                draw_length: DrawLength::Standard(0.7),
                nock_offset: 0.0,
            },
            section: {
                let limb = LimbSection {
                    alignment: LayerAlignment::SectionBack,
                    width: Width::linear(0.04, 0.01),
                    layers: vec![Layer {
                        name: "Layer 1".into(),
                        material: "Material 1".into(),
                        height: Height::linear(0.015, 0.01)
                    }],
                };
                Section {
                    materials: vec![Material {
                        name: "Material 1".into(),
                        color: "#d0b391".into(),
                        density: 675.0,
                        youngs_modulus: 12e9,
                        shear_modulus: 6e9,
                        tensile_strength: 0.0,
                        compressive_strength: 0.0,
                        safety_margin: 0.0,
                    }],
                    upper: limb.clone(),
                    lower: limb,
                }
            },
            profile: Profile {
                upper: vec![ProfileSegment::Line(Line::new(0.8))],
                lower: vec![ProfileSegment::Line(Line::new(0.8))],
            },
            string: BowString {
                n_strands: 12,
                strand_density: 0.0005,
                strand_stiffness: 3500.0,
            },
            masses: Masses {
                arrow: ArrowMass::Mass(0.025),
                limb_tip_upper: 0.0,
                limb_tip_lower: 0.0,
                string_nock: 0.0,
                string_tip_upper: 0.0,
                string_tip_lower: 0.0,
            },
            damping: Damping {
                damping_ratio_limbs: 0.05,
                damping_ratio_string: 0.05,
            },
        }
    }
}

impl TryInto<Vec<u8>> for BowModel {
    type Error = ModelError;

    // Conversion into MsgPack byte array
    fn try_into(self) -> Result<Vec<u8>, Self::Error> {
        rmp_serde::to_vec_named(&self).map_err(ModelError::InputEncodeMsgPackError)  // TODO: Bett error type?
    }
}

impl TryFrom<&[u8]> for BowModel {
    type Error = ModelError;

    // Conversion from MsgPack byte array
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        rmp_serde::from_slice(value).map_err(ModelError::InputDecodeMsgPackError)
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), ModelError> {
        let &Self { num_limb_elements, num_limb_eval_points, min_draw_resolution, max_draw_resolution, static_iteration_tolerance, arrow_clamp_force, string_compression_factor, timespan_factor, timeout_factor, min_timestep, max_timestep, steps_per_period, dynamic_iteration_tolerance} = self;

        num_limb_elements.validate_positive().map_err(ModelError::SettingsInvalidLimbElements)?;
        num_limb_eval_points.validate_at_least(2).map_err(ModelError::SettingsInvalidLimbEvalPoints)?;
        min_draw_resolution.validate_positive().map_err(ModelError::SettingsInvalidMinDrawResolution)?;
        max_draw_resolution.validate_positive().map_err(ModelError::SettingsInvalidMaxDrawResolution)?;
        static_iteration_tolerance.validate_positive().map_err(ModelError::SettingsInvalidStaticTolerance)?;

        arrow_clamp_force.validate_nonneg().map_err(ModelError::SettingsInvalidArrowClampForce)?;
        string_compression_factor.validate_positive().map_err(ModelError::SettingsInvalidStringCompressionFactor)?;
        timespan_factor.validate_at_least(1.0).map_err(ModelError::SettingsInvalidTimeSpanFactor)?;
        timeout_factor.validate_at_least(1.0).map_err(ModelError::SettingsInvalidTimeOutFactor)?;

        min_timestep.validate_positive().map_err(ModelError::SettingsInvalidMinTimeStep)?;
        max_timestep.validate_positive().map_err(ModelError::SettingsInvalidMaxTimeStep)?;
        steps_per_period.validate_positive().map_err(ModelError::SettingsInvalidStepsPerPeriod)?;
        dynamic_iteration_tolerance.validate_positive().map_err(ModelError::SettingsInvalidDynamicTolerance)?;

        Ok(())
    }
}

impl Handle {
    pub fn validate(&self) -> Result<(), ModelError> {
        match self {
            Handle::Flexible => {
                // Nothing to validate here
            }
            Handle::Rigid(RigidHandle{ length_upper, length_lower, angle, pivot }) => {
                length_upper.validate_nonneg().map_err(ModelError::HandleInvalidLength)?;
                length_lower.validate_nonneg().map_err(ModelError::HandleInvalidLength)?;
                angle.validate_finite().map_err(ModelError::HandleInvalidAngle)?;
                pivot.validate_finite().map_err(ModelError::HandleInvalidPivot)?;
            }
            Handle::Beam(BeamHandle{ length_upper, length_lower, angle, pivot, n_elements_upper, n_elements_lower, section }) => {
                length_upper.validate_positive().map_err(ModelError::HandleInvalidLength)?;
                length_lower.validate_positive().map_err(ModelError::HandleInvalidLength)?;
                angle.validate_finite().map_err(ModelError::HandleInvalidAngle)?;
                pivot.validate_finite().map_err(ModelError::HandleInvalidPivot)?;
                n_elements_upper.validate_positive().map_err(ModelError::SettingsInvalidLimbElements)?;
                n_elements_lower.validate_positive().map_err(ModelError::SettingsInvalidLimbElements)?;
                section.validate()?;
            }
        }

        Ok(())
    }

    // Determines the rigid handle parameters:
    // - If a rigid handle is already selected, just pass its parameters through
    // - A flexible handle corresponds to rigid handle length zero, angle zero and the pivot point placed at the belly
    // - A beam handle uses its geometric parameters as the equivalent rigid layout
    //   for the purpose of placing the limb roots and computing the brace/draw reference.
    pub fn to_rigid(&self) -> RigidHandle {
        match self {
            Handle::Rigid(handle) => handle.clone(),
            Handle::Flexible => RigidHandle {
                length_upper: 0.0,
                length_lower: 0.0,
                angle: 0.0,
                pivot: 0.0,
            },
            Handle::Beam(b) => RigidHandle {
                length_upper: b.length_upper,
                length_lower: b.length_lower,
                angle: b.angle,
                pivot: b.pivot,
            },
        }
    }

    /// Returns the grip cross-section of a `Handle::Beam`, or `None` for
    /// other variants.
    pub fn beam_section(&self) -> Option<&LimbSection> {
        match self {
            Handle::Beam(b) => Some(&b.section),
            _ => None,
        }
    }
}

impl Draw {
    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { brace_height, draw_length, nock_offset } = self;

        brace_height.validate_positive().map_err(ModelError::DimensionsInvalidBraceHeight)?;
        draw_length.from_pivot().validate_larger_than(*brace_height).map_err(ModelError::DimensionsInvalidDrawLength)?;
        nock_offset.validate_finite().map_err(ModelError::DimensionsInvalidBraceHeight)?;

        Ok(())
    }
}

impl DrawLength {
    // Enclosed value, independent of present enum variant
    pub fn value(&self) -> f64 {
        match *self {
            Self::Standard(value) | Self::Amo(value) => value,
        }
    }

    // Draw length as measured from the pivot point of the handle
    pub fn from_pivot(&self) -> f64 {
        match *self {
            DrawLength::Standard(value) => value,            // Already measured from pivot
            DrawLength::Amo(value) => value - 1.75*0.0254    // Subtract 1.75 inches according to AMO standard
        }
    }
}

impl Material {
    pub fn new(name: &str, color: &str, rho: f64, E: f64, G: f64) -> Self {
        Self {
            name: name.to_string(),
            color: color.to_string(),
            density: rho,
            youngs_modulus: E,
            shear_modulus: G,
            tensile_strength: 0.0,
            compressive_strength: 0.0,
            safety_margin: 0.0
        }
    }

    // Maximum stresses (tension, compression) at which the material fails
    pub fn maximum_stresses(&self) -> (f64, f64) {
        (self.tensile_strength, self.compressive_strength)
    }

    // Allowed stresses (tension, compression) according to the safety margin
    pub fn allowed_stresses(&self) -> (f64, f64) {
        let (tension, compression) = self.maximum_stresses();
        (tension*(1.0 - self.safety_margin), compression*(1.0 - self.safety_margin))
    }

    // Maximum strains (tension, compression) at which the material fails
    pub fn maximum_strains(&self) -> (f64, f64) {
        let (tension, compression) = self.maximum_stresses();
        (tension/self.youngs_modulus, compression/self.youngs_modulus)
    }

    // Allowed strains (tension, compression) according to the safety margin
    pub fn allowed_strains(&self) -> (f64, f64) {
        let (tension, compression) = self.allowed_stresses();
        (tension/self.youngs_modulus, compression/self.youngs_modulus)
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { name, color, density, youngs_modulus, shear_modulus, tensile_strength, compressive_strength, safety_margin } = self;

        name.validate_name().map_err(ModelError::MaterialInvalidName)?;
        color.validate_hex_color().map_err(|value| ModelError::MaterialInvalidColor(name.into(), value))?;

        density.validate_positive().map_err(|value| ModelError::MaterialInvalidDensity(name.into(), value))?;
        youngs_modulus.validate_positive().map_err(|value| ModelError::MaterialInvalidYoungsModulus(name.into(), value))?;
        shear_modulus.validate_positive().map_err(|value| ModelError::MaterialInvalidShearModulus(name.into(), value))?;

        tensile_strength.validate_nonneg().map_err(|value| ModelError::MaterialInvalidTensileStrength(name.into(), value))?;    // Allows values of zero for "unknown"
        compressive_strength.validate_nonneg().map_err(|value| ModelError::MaterialInvalidCompressiveStrength(name.into(), value))?;    // Allows values of zero for "unknown"
        safety_margin.validate_range_inclusive(0.0, 1.0).map_err(|value| ModelError::MaterialInvalidSafetyMargin(name.into(), value))?;

        Ok(())
    }
}

impl Layer {
    pub fn new(name: &str, material: &str, height: Height) -> Self {
        Self {
            name: name.to_string(),
            material: material.to_string(),
            height
        }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { name, material, height } = self;

        name.validate_name().map_err(ModelError::LayerInvalidName)?;
        material.validate_name().map_err(|value| ModelError::LayerInvalidMaterial(name.into(), value))?;
        height.validate(name)?;

        Ok(())
    }
}

impl Profile {
    /// Construct a symmetric profile (upper == lower).
    pub fn new(segments: Vec<ProfileSegment>) -> Self {
        Self {
            upper: segments.clone(),
            lower: segments,
        }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { upper, lower } = self;

        for (index, segment) in upper.iter().enumerate() {
            segment.validate(index)?;
        }
        for (index, segment) in lower.iter().enumerate() {
            segment.validate(index)?;
        }

        Ok(())
    }

    /// Returns the segment list for the requested limb side.
    pub fn for_side(&self, side: LimbSide) -> &Vec<ProfileSegment> {
        match side {
            LimbSide::Upper => &self.upper,
            LimbSide::Lower => &self.lower,
        }
    }
}

impl LimbSection {
    pub fn new(alignment: LayerAlignment, width: Width, layers: Vec<Layer>) -> Self {
        Self { alignment, width, layers }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { alignment, width, layers } = self;

        match alignment {
            LayerAlignment::LayerBack(name) | LayerAlignment::LayerBelly(name) | LayerAlignment::LayerCenter(name) => {
                name.validate_name().map_err(ModelError::ProfileAnlignemtInvalidLayerName)?;
            }
            _ => { }
        }

        width.validate()?;
        for layer in layers {
            layer.validate()?;
        }

        Ok(())
    }
}

impl Section {
    /// Construct a symmetric section (upper == lower).
    pub fn new(alignment: LayerAlignment, width: Width, materials: Vec<Material>, layers: Vec<Layer>) -> Self {
        let limb = LimbSection { alignment, width, layers };
        Self {
            materials,
            upper: limb.clone(),
            lower: limb,
        }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { materials, upper, lower } = self;

        for material in materials {
            material.validate()?;
        }

        upper.validate()?;
        lower.validate()?;

        Ok(())
    }

    /// Returns the per-limb section data for the requested side.
    pub fn for_side(&self, side: LimbSide) -> &LimbSection {
        match side {
            LimbSide::Upper => &self.upper,
            LimbSide::Lower => &self.lower,
        }
    }
}

impl Width {
    pub fn new(points: Vec<[f64; 2]>) -> Self {
        Self(points)
    }

    // Convenience function for creating a constant width distribution
    pub fn constant(w: f64) -> Self {
        Self::new(vec![
            [0.0, w],
            [1.0, w]
        ])
    }

    // Convenience function for creating a linear width distribution
    pub fn linear(w0: f64, w1: f64) -> Self {
        Self::new(vec![
            [0.0, w0],
            [1.0, w1]
        ])
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        let Self( points ) = self;

        // At least two control points are required
        points.len().validate_at_least(2).map_err(ModelError::WidthControlPointsTooFew)?;

        // The control points must be sorted by strictly increasing position
        if let Some((a, b)) = points.iter().tuple_windows().find(|(a, b)| b[0] <= a[0]) {
            return Err(ModelError::WidthControlPointsNotSorted(a[0], b[0]));
        }

        let first = points.first().unwrap();    // Unwrap okay because of previous validation
        let last = points.last().unwrap();      // Unwrap okay because of previous validation

        // The control points must cover the range 0 to 1 exactly
        first[0].validate_equals(0.0).map_err(|_| ModelError::WidthControlPointsInvalidRange(first[0], last[0]))?;
        last[0].validate_equals(1.0).map_err(|_| ModelError::WidthControlPointsInvalidRange(first[0], last[0]))?;

        // The widths contained in the control points must be strictly positive
        if let Some(a) = points.iter().find(|a| !a[0].is_finite() || !a[1].is_finite() || a[1] <= 0.0) {
            return Err(ModelError::WidthControlPointsInvalidValue(a[0], a[1]));
        }

        Ok(())
    }
}

impl Height {
    pub fn new(points: Vec<[f64; 2]>) -> Self {
        Self(points)
    }

    // Convenience function for creating a constant width distribution
    pub fn constant(h: f64) -> Self {
        Self::new(vec![
            [0.0, h],
            [1.0, h]
        ])
    }

    // Convenience function for creating a linear width distribution
    pub fn linear(h0: f64, h1: f64) -> Self {
        Self::new(vec![
            [0.0, h0],
            [1.0, h1]
        ])
    }

    pub fn validate(&self, name: &str) -> Result<(), ModelError> {
        let Self( points ) = self;

        // At least two control points are required
        points.len().validate_at_least(2).map_err(|len| ModelError::LayerHeightControlPointsTooFew(name.into(), len))?;

        // Control points must be sorted by strictly increasing position
        if let Some((a, b)) = points.iter().tuple_windows().find(|(a, b)| b[0] <= a[0]) {
            return Err(ModelError::LayerHeightControlPointsNotSorted(name.into(), a[0], b[0]));
        }

        let first = points.first().unwrap();    // Unwrap okay because of previous validation
        let last = points.last().unwrap();      // Unwrap okay because of previous validation

        // First control point must lie within the range 0 to 1 and its height must be positive or zero
        first[0].validate_range_inclusive(0.0, 1.0).map_err(|_| ModelError::LayerHeightControlPointsInvalidRange(name.into(), first[0], last[0]))?;
        first[1].validate_nonneg().map_err(|_| ModelError::LayerHeightControlPointsInvalidBoundaryValue(name.into(), first[0], first[1]))?;

        // Last control point must lie within the range 0 to 1 and its height must be positive or zero
        last[0].validate_range_inclusive(0.0, 1.0).map_err(|_| ModelError::LayerHeightControlPointsInvalidRange(name.into(), first[0], last[0]))?;
        last[1].validate_nonneg().map_err(|_| ModelError::LayerHeightControlPointsInvalidBoundaryValue(name.into(), last[0], last[1]))?;

        // Other non-boundary points must have s positive height
        for point in points.iter().skip(1).take(points.len() - 2) {
            point[1].validate_positive().map_err(|_| ModelError::LayerHeightControlPointsInvalidInteriorValue(name.into(), point[0], point[1]))?;
        }

        // If the first control point is not at position zero, it must have a height of zero for continuity reasons
        if first[0] > 0.0 && first[1] != 0.0 {
            return Err(ModelError::LayerHeightControlPointsDiscontinuousBoundary(name.into(), first[0], first[1]));
        }

        // If the last control point is not at position 1, it must have a height of zero for continuity reasons
        if last[0] < 1.0 && last[1] != 0.0 {
            return Err(ModelError::LayerHeightControlPointsDiscontinuousBoundary(name.into(), last[0], last[1]));
        }

        Ok(())
    }
}

impl BowString {
    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { n_strands, strand_density, strand_stiffness } = self;
        n_strands.validate_positive().map_err(ModelError::StringInvalidNumberOfStrands)?;
        strand_density.validate_positive().map_err(ModelError::StringInvalidStrandDensity)?;
        strand_stiffness.validate_positive().map_err(ModelError::StringInvalidStrandStiffness)?;

        Ok(())
    }
}

impl Masses {
    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { arrow, limb_tip_upper, limb_tip_lower, string_nock, string_tip_upper, string_tip_lower } = self;
        match &arrow {
            ArrowMass::Mass(mass) => mass.validate_positive().map_err(ModelError::MassesInvalidArrowMass)?,
            ArrowMass::MassPerForce(mass) => mass.validate_positive().map_err(ModelError::MassesInvalidArrowMassPerForce)?,
            ArrowMass::MassPerEnergy(mass) => mass.validate_positive().map_err(ModelError::MassesInvalidArrowMassPerEnergy)?,
        }
        limb_tip_upper.validate_nonneg().map_err(ModelError::MassesInvalidLimbTipMass)?;
        limb_tip_lower.validate_nonneg().map_err(ModelError::MassesInvalidLimbTipMass)?;
        string_nock.validate_nonneg().map_err(ModelError::MassesInvalidStringCenterMass)?;
        string_tip_upper.validate_nonneg().map_err(ModelError::MassesInvalidStringTipMass)?;
        string_tip_lower.validate_nonneg().map_err(ModelError::MassesInvalidStringTipMass)?;

        Ok(())
    }

    pub fn limb_tip(&self, side: LimbSide) -> f64 {
        match side {
            LimbSide::Upper => self.limb_tip_upper,
            LimbSide::Lower => self.limb_tip_lower,
        }
    }

    pub fn string_tip(&self, side: LimbSide) -> f64 {
        match side {
            LimbSide::Upper => self.string_tip_upper,
            LimbSide::Lower => self.string_tip_lower,
        }
    }
}

impl Damping {
    pub fn validate(&self) -> Result<(), ModelError> {
        let Self { damping_ratio_limbs, damping_ratio_string } = self;
        damping_ratio_limbs.validate_range_inclusive(0.0, 1.0).map_err(ModelError::DampingInvalidLimbDampingRatio)?;
        damping_ratio_string.validate_range_inclusive(0.0, 1.0).map_err(ModelError::DampingInvalidStringDampingRatio)?;

        Ok(())
    }
}

impl ProfileSegment {
    pub fn validate(&self, index: usize) -> Result<(), ModelError> {
        match self {
            ProfileSegment::Line(input)   => input.validate(index),
            ProfileSegment::Arc(input)    => input.validate(index),
            ProfileSegment::Spiral(input) => input.validate(index),
            ProfileSegment::Spline(input) => input.validate(index),
        }
    }
}

impl Line {
    pub fn new(length: f64) -> Self {
        Self {
            length
        }
    }

    pub fn validate(&self, index: usize) -> Result<(), ModelError> {
        let &Self { length } = self;
        length.validate_positive().map_err(|_| ModelError::LineSegmentInvalidLength(index, length))?;

        Ok(())
    }
}

impl Arc {
    pub fn new(length: f64, radius: f64) -> Self {
        Self {
            length,
            radius,
        }
    }

    pub fn validate(&self, index: usize) -> Result<(), ModelError> {
        let &Self { length, radius } = self;
        length.validate_positive().map_err(|_| ModelError::ArcSegmentInvalidLength(index, length))?;
        radius.validate_finite().map_err(|_| ModelError::ArcSegmentInvalidRadius(index, length))?;

        Ok(())
    }
}

impl Spiral {
    pub fn new(length: f64, radius0: f64, radius1: f64) -> Self {
        Self {
            length,
            radius_start: radius0,
            radius_end: radius1
        }
    }

    pub fn validate(&self, index: usize) -> Result<(), ModelError> {
        let &Self { length, radius_start: radius0, radius_end: radius1 } = self;
        length.validate_positive().map_err(|_| ModelError::SpiralSegmentInvalidLength(index, length))?;
        radius0.validate_finite().map_err(|_| ModelError::SpiralSegmentInvalidRadius0(index, length))?;
        radius1.validate_finite().map_err(|_| ModelError::SpiralSegmentInvalidRadius1(index, length))?;

        Ok(())
    }
}

impl Spline {
    pub fn new(points: Vec<[f64; 2]>) -> Self {
        Self {
            points
        }
    }

    pub fn validate(&self, index: usize) -> Result<(), ModelError> {
        let Self { points } = self;

        points.len().validate_at_least(2).map_err(|_| ModelError::SplineSegmentTooFewPoints(index, points.len()))?;

        for point in points {
            point[0].validate_finite().map_err(|_| ModelError::SplineSegmentInvalidPoint(index, *point))?;
            point[1].validate_finite().map_err(|_| ModelError::SplineSegmentInvalidPoint(index, *point))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::path::PathBuf;
    use itertools::Itertools;
    use assert2::assert;
    use assert_matches::assert_matches;
    use serde::Serialize;
    use serde_json::{json, Value};

    #[test]
    fn test_model_conversion() {
        // This test loads bow files that are supposed to be equivalent but have been saved in different file input.
        // It checks whether they can be loaded/saved successfully and if the resulting model data is equivalent.
        // See also the readme file in the referenced folder.
        for entry in std::fs::read_dir("data/versions").unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                println!("{path:?}");
                check_version_folder(path);
            }
        }
    }

    // Finds and loads all .bow files in a given directory and returns the file paths and model data
    fn load_models_from_dir<P: AsRef<Path>>(path: P) -> Vec<(PathBuf, BowModel)> {
        std::fs::read_dir(&path).unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .filter(|path| path.extension().map(|s| s == "bow").unwrap())
            .filter(|path| path.file_name().map(|s| s != "latest.bow").unwrap())
            .map(|file| {
                let model = BowModel::load(&file).unwrap_or_else(|_| panic!("Failed to load model {file:?}"));
                return (file, model);
            })
            .collect()
    }

    // Performs loading and conversion checks for the model files found in the given directory
    fn check_version_folder<P: AsRef<Path>>(path: P) {
        // Check 1: Load all models from the folder, which verifies that they can be loaded and converted
        let models = load_models_from_dir(&path);
        assert!(!models.is_empty());

        // Check 2: The resulting model data after conversion must be equal for each file
        for ((file_a, model_a), (file_b, model_b)) in models.iter().tuple_windows() {
            assert!(model_a == model_b, "Model data of {:?} and {:?} is not equal", file_a, file_b);
        }

        // Save model data in the latest version
        let (_, model) = &models[0];
        let file = path.as_ref().join("latest.bow");
        model.save(&file).unwrap_or_else(|_| panic!("Failed to save model {file:?}"));

        // Load it again and check for equality
        let loaded = BowModel::load(&file).unwrap_or_else(|_| panic!("Failed to load model {file:?}"));
        assert!(loaded == *model, "Model data of {:?} must be equal to its source", file);

        // Load it once more as a Json Value and check if the version entry matches the Cargo package version
        let mut reader = File::open(&file).unwrap_or_else(|_| panic!("Failed to load file {file:?}"));
        let value: Value = serde_json::from_reader(&mut reader).unwrap_or_else(|_| panic!("Failed to parse file {file:?}"));
        let version = value.get("version").unwrap_or_else(|| panic!("Model {file:?} has no version entry"));
        assert!(version == &json!(env!("CARGO_PKG_VERSION")), "Version of model {:?} does not match the Cargo package version", file);
    }

    #[test]
    fn test_load_model() {
        generate_test_files();

        // IO error when loading from an invalid path
        assert_matches!(BowModel::load("data/input/nonexistent.bow"), Err(ModelError::InputLoadFileError(_, _)));

        // Deserialization error due to the file containing invalid json
        assert_matches!(BowModel::load("data/input/invalid_json.bow"), Err(ModelError::InputDeserializeJsonError(_)));

        // Deserialization error due to the file containing valid json but no version entry
        assert_matches!(BowModel::load("data/input/version_missing.bow"), Err(ModelError::InputDeserializeJsonError(_)));

        // Deserialization error due to the file containing valid json but an invalid version entry (wrong type)
        assert_matches!(BowModel::load("data/input/version_invalid.bow"), Err(ModelError::InputDeserializeJsonError(_)));

        // Error when loading a bow file with a version that is unsupported (too old)
        assert_matches!(BowModel::load("data/input/version_unsupported.bow"), Err(ModelError::InputVersionUnsupported));

        // Error when loading a bow file with a version that is not recognized
        assert_matches!(BowModel::load("data/input/version_unrecognized.bow"), Err(ModelError::InputVersionUnrecognized));

        // Deserialization error due to invalid file contents (valid json and version but invalid structure)
        assert_matches!(BowModel::load("data/input/invalid_content.bow"), Err(ModelError::InputDeserializeJsonError(_)));

        // No error when loading a valid bow model
        assert_matches!(BowModel::load("data/input/valid_model.bow"), Ok(_));
    }

    #[test]
    fn test_save_model() {
        let model = BowModel::example();

        // IO error from saving to an invalid path
        assert_matches!(model.save("data/input/nonexistent/valid.bow"), Err(ModelError::InputSaveFileError(_, _)));

        // The only remaining error case is a serialization error, but that one is difficult to trigger.
        // Saving without error is already covered by the generation of the test files above.
    }

    fn generate_test_files() {
        // File that contains invalid json content, in this case just an empty fie
        File::create("data/input/invalid_json.bow").unwrap();

        // File with valid json but no version entry
        let mut file = File::create("data/input/version_missing.bow").unwrap();
        let data = NoVersion { x: 1.0, y: 2.0, z: 3.0, };
        serde_json::to_writer_pretty(&mut file, &data).unwrap();

        // File with valid json but invalid version entry (wrong type)
        let mut file = File::create("data/input/version_invalid.bow").unwrap();
        let data = VersionUsize { version: 7, x: 1.0, y: 2.0, z: 3.0, };
        serde_json::to_writer_pretty(&mut file, &data).unwrap();

        // File with valid json and valid version entry but the version is unsupported (too old)
        let mut file = File::create("data/input/version_unsupported.bow").unwrap();
        let data = VersionString { version: "0.3".to_string(), x: 1.0, y: 2.0, z: 3.0, };
        serde_json::to_writer_pretty(&mut file, &data).unwrap();

        // File with valid json and valid version entry but the version is unknown
        let mut file = File::create("data/input/version_unrecognized.bow").unwrap();
        let data = VersionString { version: "xyz".to_string(), x: 1.0, y: 2.0, z: 3.0, };
        serde_json::to_writer_pretty(&mut file, &data).unwrap();

        // File that contains valid json with matching version but not a valid bow model
        let mut file = File::create("data/input/invalid_content.bow").unwrap();
        let data = VersionString { version: env!("CARGO_PKG_VERSION").to_string(), x: 1.0, y: 2.0, z: 3.0, };
        serde_json::to_writer_pretty(&mut file, &data).unwrap();

        // File that contains valid bow model data the correct version
        let model = BowModel::example();
        model.save("data/input/valid_model.bow").unwrap();
    }

    #[derive(Serialize)]
    struct NoVersion {
        x: f64,
        y: f64,
        z: f64
    }

    #[derive(Serialize)]
    struct VersionUsize {
        version: usize,
        x: f64,
        y: f64,
        z: f64
    }

    #[derive(Serialize)]
    struct VersionString {
        version: String,
        x: f64,
        y: f64,
        z: f64
    }
}