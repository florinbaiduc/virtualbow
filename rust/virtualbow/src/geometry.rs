// Bow geometry. v5 onwards represents the full bow as TWO independent
// `LimbHalfGeometry` chains (upper / lower) that share a grip / handle.
//
// Each half is built in a "half-bow local frame" where the limb starts at the
// grip (handle pivot) and extends along +x toward its tip.  When constructing
// the FEM model, the simulation places the upper limb in world +x and the
// lower limb in world -x (its x-coordinates are negated and its in-plane
// rotation is reflected accordingly).

use iter_num_tools::lin_space;
use itertools::Itertools;
use nalgebra::{DVector, SVector, vector};
use serde::{Deserialize, Serialize};
use crate::errors::ModelError;
use crate::input::{BowModel, DrawLength, LimbSide};
use crate::profile::profile::{CurvePoint, ProfileCurve};
use crate::sections::section::LayeredCrossSection;
use virtualbow_num::fem::elements::beam::geometry::{CrossSection, PlanarCurve};
use virtualbow_num::fem::elements::beam::linear::LinearBeamSegment;
use crate::output::LimbInfo;

/// Continuous (pre-discretization) geometry for a single limb half.
pub struct LimbHalfGeometry {
    pub profile: ProfileCurve,
    pub section: LayeredCrossSection,
    pub side: LimbSide,
}

/// Continuous geometry for the whole bow.
pub struct BowGeometry {
    pub upper: LimbHalfGeometry,
    pub lower: LimbHalfGeometry,
    pub draw: DrawInfo,
}

#[derive(Serialize, Deserialize, Default, PartialEq, Debug, Clone)]
pub struct DiscreteLimbHalfGeometry {
    pub side: LimbSide,
    pub segments: Vec<LinearBeamSegment>,
    pub n_nodes: Vec<f64>,
    pub s_nodes: Vec<f64>,
    pub k_nodes: Vec<f64>,
    pub p_nodes: Vec<SVector<f64, 3>>,
    pub y_nodes: Vec<DVector<f64>>,
    pub h_nodes: Vec<DVector<f64>>,

    pub p_control: Vec<SVector<f64, 3>>,

    pub n_eval: Vec<f64>,
    pub s_eval: Vec<f64>,
    pub k_eval: Vec<f64>,
    pub p_eval: Vec<SVector<f64, 3>>,
    pub y_eval: Vec<DVector<f64>>,
    pub h_eval: Vec<DVector<f64>>,
    pub w_eval: Vec<f64>,

    pub strain_eval: Vec<Vec<SVector<f64, 3>>>,
    pub stress_eval: Vec<Vec<SVector<f64, 3>>>,
}

#[derive(Serialize, Deserialize, Default, PartialEq, Debug, Clone)]
pub struct DiscreteBowGeometry {
    pub upper: DiscreteLimbHalfGeometry,
    pub lower: DiscreteLimbHalfGeometry,
    pub draw: DrawInfo,
}

#[derive(Serialize, Deserialize, Default, PartialEq, Debug, Clone)]
pub struct DrawInfo {
    pub pivot_point: f64,                // Position of the pivot point (y in world frame)
    pub brace_ref: f64,
    pub draw_ref: f64,
    pub brace_pos: f64,
    pub draw_pos: f64,
    pub power_stroke: f64,
    /// Signed offset of the nock point from the bow's geometric center along
    /// the bow's longitudinal (x) axis. Positive = toward the upper limb.
    pub nock_offset: f64,
}

impl Default for LimbSide {
    fn default() -> Self { LimbSide::Upper }
}

impl LimbHalfGeometry {
    /// Build the continuous geometry of one limb half (in its local frame:
    /// the limb starts at the grip on the y-axis and extends along +x).
    /// Self-intersection checking is done here.
    pub fn new(input: &BowModel, side: LimbSide) -> Result<Self, ModelError> {
        // Per-limb section
        let limb_section = input.section.for_side(side);
        let section = LayeredCrossSection::new(limb_section, &input.section.materials)?;

        // Determine rigid parameters of the handle for this side.
        // Each side gets its own half of the handle length.
        let rigid_handle = input.handle.to_rigid();
        let half_length = match side {
            LimbSide::Upper => rigid_handle.length_upper,
            LimbSide::Lower => rigid_handle.length_lower,
        };

        // Profile curve for this side.  Its starting point is the corresponding
        // tip of the rigid handle, advancing along +x in the local frame.
        let start = CurvePoint::new(0.0, rigid_handle.angle, vector![half_length, 0.0]);
        let profile = ProfileCurve::new(start, input.profile.for_side(side))?;

        // Self-intersection checks
        for s in lin_space(profile.length_start()..=profile.length_end(), 1000) {
            let kappa = profile.curvature(s);
            let (y_back, y_belly) = section.section_bounds(profile.normalize(s));

            if kappa > 0.0 && y_back >= 1.0/kappa {
                return Err(ModelError::GeometrySelfIntersectionBack(s));
            }
            else if kappa < 0.0 && y_belly <= 1.0/kappa {
                return Err(ModelError::GeometrySelfIntersectionBelly(s));
            }
        }

        Ok(Self { profile, section, side })
    }

    pub fn discretize(&self, n_eval_points: usize, n_elements: usize) -> DiscreteLimbHalfGeometry {
        // Always build the discrete data in the limb's LOCAL frame (limb extends
        // along +x from its grip).  For the lower limb we then transform every
        // segment / position into the WORLD frame by applying the reflection
        //   T_pos: (x, y, φ) → (-x, y, π - φ)
        // and the corresponding 6×6 similarity transform on the segment K
        //   T6 = diag(-1, 1, -1, -1, 1, -1)    →    K_world = T6 · K_local · T6
        // This is the correct (and provably mirror-exact) way to express the
        // lower limb in the world frame, regardless of how the cross-section
        // varies along the limb.
        let local = self.discretize_with(&self.profile, n_eval_points, n_elements);
        match self.side {
            LimbSide::Upper => local,
            LimbSide::Lower => mirror_discrete_to_world(local),
        }
    }

    fn discretize_with<C: PlanarCurve>(&self, curve: &C, n_eval_points: usize, n_elements: usize) -> DiscreteLimbHalfGeometry {
        let s_nodes = lin_space(curve.length_start()..=curve.length_end(), n_elements + 1).collect_vec();
        let n_nodes = s_nodes.iter().map(|&s| curve.normalize(s)).collect_vec();
        let k_nodes = s_nodes.iter().map(|&s| curve.curvature(s)).collect_vec();
        let p_nodes = s_nodes.iter().map(|&s| curve.point(s)).collect_vec();
        let y_nodes = n_nodes.iter().map(|&n| self.section.layer_bounds(n).0).collect_vec();
        let h_nodes = n_nodes.iter().map(|&n| self.section.layer_bounds(n).1).collect_vec();

        // Profile control points (always in local frame; mirroring happens in
        // `mirror_discrete_to_world` for the lower limb).
        let p_control: Vec<SVector<f64, 3>> = self.profile.get_nodes().iter().map(|node| {
            vector![node.r[0], node.r[1], node.φ]
        }).collect();

        let s_eval = lin_space(curve.length_start()..=curve.length_end(), n_eval_points).collect_vec();
        let n_eval = s_eval.iter().map(|&s| curve.normalize(s)).collect_vec();
        let y_eval = n_eval.iter().map(|&n| self.section.layer_bounds(n).0).collect_vec();
        let h_eval = n_eval.iter().map(|&n| self.section.layer_bounds(n).1).collect_vec();

        let segments = s_nodes.iter().tuple_windows().enumerate().map(|(i, (&s0, &s1))| {
            let tolerance = 1e-9;
            let s_eval = if i == 0 {
                s_eval.iter().copied().filter(|&s| s >= s0 - tolerance && s <= s1 ).collect_vec()
            }
            else if i == n_elements - 1 {
                s_eval.iter().copied().filter(|&s| s > s0 && s <= s1 + tolerance ).collect_vec()
            }
            else {
                s_eval.iter().copied().filter(|&s| s > s0 && s <= s1 ).collect_vec()
            };

            LinearBeamSegment::new(curve, &self.section, s0, s1, &s_eval)
        }).collect();

        let k_eval = s_eval.iter().map(|&s| curve.curvature(s)).collect_vec();
        let p_eval = s_eval.iter().map(|&s| curve.point(s)).collect();
        let w_eval = n_eval.iter().map(|&n| self.section.width(n)).collect();

        let strain_eval = n_eval.iter().map(|&n| self.section.strain_recovery(n)).collect();
        let stress_eval = n_eval.iter().map(|&n| self.section.stress_recovery(n)).collect();

        DiscreteLimbHalfGeometry {
            side: self.side,
            segments,
            n_nodes,
            s_nodes,
            k_nodes,
            p_nodes,
            y_nodes,
            h_nodes,
            p_control,
            n_eval,
            s_eval,
            y_eval,
            strain_eval,
            stress_eval,
            k_eval,
            p_eval,
            w_eval,
            h_eval,
        }
    }
}

/// Reflects a `DiscreteLimbHalfGeometry` from the limb's local frame
/// (extending +x from the grip) into the world frame for the lower limb
/// (extending -x from the grip).  Position-like data has its x-component
/// negated and φ becomes π - φ; per-segment stiffness matrices undergo
/// a similarity transform K' = T6 · K · T6 with T6 = diag(-1, 1, -1, -1, 1, -1).
fn mirror_discrete_to_world(mut g: DiscreteLimbHalfGeometry) -> DiscreteLimbHalfGeometry {
    use std::f64::consts::PI;
    use nalgebra::SMatrix;

    let mirror_p3 = |p: &SVector<f64, 3>| vector![-p[0], p[1], PI - p[2]];

    // Mirror node and eval positions and the profile control polyline.
    for p in g.p_nodes.iter_mut()    { *p = mirror_p3(p); }
    for p in g.p_eval.iter_mut()     { *p = mirror_p3(p); }
    for p in g.p_control.iter_mut()  { *p = mirror_p3(p); }

    // Mirror curvature (chirality flip).
    for k in g.k_nodes.iter_mut() { *k = -*k; }
    for k in g.k_eval.iter_mut()  { *k = -*k; }

    // T6 similarity transform on every segment K, plus mirror p0/p1/pe.
    let mut t6 = SMatrix::<f64, 6, 6>::zeros();
    for (i, s) in [-1.0, 1.0, -1.0, -1.0, 1.0, -1.0].iter().enumerate() {
        t6[(i, i)] = *s;
    }
    let mut t3 = SMatrix::<f64, 3, 3>::zeros();
    for (i, s) in [-1.0, 1.0, -1.0].iter().enumerate() {
        t3[(i, i)] = *s;
    }
    // 3×6 block-diag for evaluation matrices: each acts on a 6-DOF input
    // (two-node displacements) and produces a 3-DOF output (cross-section
    // displacements / forces).  Under mirror, both input and output transform.
    for seg in g.segments.iter_mut() {
        seg.p0 = mirror_p3(&seg.p0);
        seg.p1 = mirror_p3(&seg.p1);
        for p in seg.pe.iter_mut() { *p = mirror_p3(p); }
        seg.K = t6 * seg.K * t6;
        for e in seg.Ep.iter_mut() { *e = t3 * (*e) * t6; }
        for e in seg.Ef.iter_mut() { *e = t3 * (*e) * t6; }
        // Ci (3×3 inverse cross-section stiffness): cross-section frame is
        // independent of the limb orientation, so no transform needed —
        // EXCEPT we used the local s, hence local cross-section orientation,
        // which is unchanged.  (The lumped mass M is just diagonal masses.)
    }

    g
}

impl BowGeometry {
    pub fn new(input: &BowModel) -> Result<Self, ModelError> {
        let upper = LimbHalfGeometry::new(input, LimbSide::Upper)?;
        let lower = LimbHalfGeometry::new(input, LimbSide::Lower)?;

        // Draw / brace info is derived from the upper limb's section & the
        // (shared) handle parameters.  Both limbs use the same global y-axis
        // for the string motion.
        let rigid_handle = input.handle.to_rigid();
        let eccentricity = upper.section.section_bounds(0.0).1;
        let pivot_point = eccentricity*f64::cos(rigid_handle.angle) - rigid_handle.pivot;
        let brace_ref = pivot_point;
        let draw_ref = match input.draw.draw_length {
            DrawLength::Standard(_) => pivot_point,
            DrawLength::Amo(_) => pivot_point + 1.75*0.0254
        };
        let brace_pos = brace_ref - input.draw.brace_height;
        let draw_pos = draw_ref - input.draw.draw_length.value();
        let power_stroke = brace_pos - draw_pos;

        let draw = DrawInfo {
            pivot_point,
            brace_ref,
            draw_ref,
            brace_pos,
            draw_pos,
            power_stroke,
            nock_offset: input.draw.nock_offset,
        };

        Ok(Self { upper, lower, draw })
    }

    pub fn discretize(&self, n_eval_points: usize, n_elements: usize) -> DiscreteBowGeometry {
        DiscreteBowGeometry {
            upper: self.upper.discretize(n_eval_points, n_elements),
            lower: self.lower.discretize(n_eval_points, n_elements),
            draw: self.draw.clone(),
        }
    }
}

impl DiscreteLimbHalfGeometry {
    pub fn to_limb_info(&self) -> LimbInfo {
        LimbInfo {
            length: self.s_eval.clone(),
            position_eval: self.p_eval.clone(),
            position_control: self.p_control.clone(),
            curvature_eval: self.k_eval.clone(),
            width: self.w_eval.clone(),
            height: self.h_eval.iter().map(|h| h.sum()).collect(),
            bounds: self.y_eval.iter().map(|y| y.data.clone().into()).collect(),
            ratio: self.n_eval.clone(),
            heights: self.h_eval.iter().map(|h| h.data.clone().into()).collect(),
        }
    }
}

impl DiscreteBowGeometry {
    pub fn to_bow_info(&self) -> crate::output::BowInfo {
        crate::output::BowInfo {
            upper: self.upper.to_limb_info(),
            lower: self.lower.to_limb_info(),
            pivot_point: self.draw.pivot_point,
            nock_offset: self.draw.nock_offset,
        }
    }
}

impl TryInto<Vec<u8>> for crate::output::BowInfo {
    type Error = ModelError;

    fn try_into(self) -> Result<Vec<u8>, Self::Error> {
        rmp_serde::to_vec_named(&self).map_err(ModelError::OutputEncodeMsgPackError)
    }
}

impl TryFrom<&[u8]> for crate::output::BowInfo {
    type Error = ModelError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        rmp_serde::from_slice(value).map_err(ModelError::OutputDecodeMsgPackError)
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;
    use std::fmt::{Debug, Formatter};
    use crate::input::{Arc, Height, Layer, Material, Line, Profile, ProfileSegment, Section, LayerAlignment, Width};
    use super::*;

    impl Debug for LimbHalfGeometry {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "LimbHalfGeometry")
        }
    }

    impl Debug for BowGeometry {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "BowGeometry")
        }
    }

    #[test]
    fn test_error_conditions() {
        let mut input = BowModel {
            section: Section::new(
                LayerAlignment::SectionCenter,
                Width::linear(0.04, 0.01),
                vec![Material::new("Unnamed", "#000000", 600.0, 12e9, 6e9)],
                vec![Layer::new("Default", "Unnamed", Height::constant(0.01))]
            ),
            ..BowModel::example()
        };

        // 1. Profile curve with no self-intersection
        input.profile = Profile::new(vec![ProfileSegment::Line(Line::new(1.0))]);
        assert_matches!(BowGeometry::new(&input), Ok(_));

        // 2. Profile that produces a self-intersection at the back
        input.profile = Profile::new(vec![ProfileSegment::Arc(Arc::new(1.0, 0.001))]);
        assert_matches!(BowGeometry::new(&input), Err(ModelError::GeometrySelfIntersectionBack(0.0)));

        // 3. Profile that produces a self-intersection at the belly
        input.profile = Profile::new(vec![ProfileSegment::Arc(Arc::new(1.0, -0.001))]);
        assert_matches!(BowGeometry::new(&input), Err(ModelError::GeometrySelfIntersectionBelly(0.0)));
    }

    /// Verify the lower limb's segments are the world-frame mirror image of
    /// the upper limb's, in particular K_lower = T·K_upper·T with
    /// T = diag(-1, 1, -1, -1, 1, -1).
    #[test]
    fn test_mirror_symmetry() {
        use nalgebra::SMatrix;

        let input = BowModel {
            section: Section::new(
                LayerAlignment::SectionCenter,
                Width::linear(0.04, 0.01),
                vec![Material::new("Unnamed", "#000000", 600.0, 12e9, 6e9)],
                vec![Layer::new("Default", "Unnamed", Height::constant(0.01))]
            ),
            profile: Profile::new(vec![ProfileSegment::Line(Line::new(1.0))]),
            ..BowModel::example()
        };

        let geometry = BowGeometry::new(&input).unwrap().discretize(50, 5);

        // T6 = diag(-1, 1, -1, -1, 1, -1)
        let mut t6 = SMatrix::<f64, 6, 6>::zeros();
        for (i, s) in [-1.0, 1.0, -1.0, -1.0, 1.0, -1.0].iter().enumerate() {
            t6[(i, i)] = *s;
        }

        for (i, (su, sl)) in geometry.upper.segments.iter()
            .zip(geometry.lower.segments.iter()).enumerate()
        {
            // Node positions: world-frame mirror
            let pu0 = su.p0;
            let pl0 = sl.p0;
            // Reflection: x → -x, y → y, φ → π - φ
            let dx = pu0[0] + pl0[0];
            let dy = pu0[1] - pl0[1];
            let mut dphi = pu0[2] + pl0[2] - std::f64::consts::PI;
            dphi = dphi.rem_euclid(std::f64::consts::TAU);
            if dphi > std::f64::consts::PI { dphi -= std::f64::consts::TAU; }
            assert!(dx.abs() < 1e-10, "seg {} p0.x: upper={}, lower={}", i, pu0[0], pl0[0]);
            assert!(dy.abs() < 1e-10, "seg {} p0.y: upper={}, lower={}", i, pu0[1], pl0[1]);
            assert!(dphi.abs() < 1e-10, "seg {} p0.φ: upper={}, lower={} (residual = {})", i, pu0[2], pl0[2], dphi);

            // K matrix: K_lower must equal T6 · K_upper · T6
            let expected = t6 * su.K * t6;
            let diff = sl.K - expected;
            let max_err = diff.iter().fold(0.0_f64, |a, &b| a.max(b.abs()));
            let max_val = su.K.iter().fold(0.0_f64, |a, &b| a.max(b.abs()));
            assert!(
                max_err < 1e-10 * max_val.max(1.0),
                "seg {} K asymmetry: max_err = {} (vs |K|_max = {})\nupper.K =\n{}\nlower.K =\n{}\nexpected (T·U·T) =\n{}",
                i, max_err, max_val, su.K, sl.K, expected,
            );
        }
    }

    /// Verify that a full system built with two mirror-symmetric limbs gives
    /// a mirror-symmetric global stiffness matrix when no external forces or
    /// boundary conditions are applied (other than the limb root locks).
    #[test]
    fn test_full_system_mirror_symmetric() {
        use nalgebra::{DMatrix, DVector};
        use virtualbow_num::fem::elements::beam::beam::BeamElement;
        use virtualbow_num::fem::system::dof::DofType;
        use virtualbow_num::fem::system::system::System;

        let input = BowModel {
            section: Section::new(
                LayerAlignment::SectionCenter,
                Width::linear(0.04, 0.01),
                vec![Material::new("Unnamed", "#000000", 600.0, 12e9, 6e9)],
                vec![Layer::new("Default", "Unnamed", Height::constant(0.01))]
            ),
            profile: Profile::new(vec![ProfileSegment::Line(Line::new(1.0))]),
            ..BowModel::example()
        };

        let geom = BowGeometry::new(&input).unwrap().discretize(10, 4);

        let mut system = System::new();

        // Create FEM nodes (root locked) for each limb.
        let upper_nodes: Vec<_> = geom.upper.p_nodes.iter().enumerate().map(|(i, p)| {
            system.create_node(p, &[DofType::active_if(i != 0); 3])
        }).collect();
        let lower_nodes: Vec<_> = geom.lower.p_nodes.iter().enumerate().map(|(i, p)| {
            system.create_node(p, &[DofType::active_if(i != 0); 3])
        }).collect();

        for (i, seg) in geom.upper.segments.iter().enumerate() {
            system.add_element(&[upper_nodes[i], upper_nodes[i + 1]], BeamElement::new(seg));
        }
        for (i, seg) in geom.lower.segments.iter().enumerate() {
            system.add_element(&[lower_nodes[i], lower_nodes[i + 1]], BeamElement::new(seg));
        }

        let n = system.n_dofs();
        let mut q = DVector::zeros(n);
        let mut k_global = DMatrix::zeros(n, n);

        // At u = 0 (system at initial positions): residual should be zero,
        // K should be symmetric in itself (trivially, since linear elastic).
        system.compute_internal_forces(Some(&mut q), Some(&mut k_global), None);

        // Build a mirror-symmetric perturbation: for each upper.node[i] active,
        // pick (du_x, du_y, du_φ); for the corresponding lower.node[i],
        // set (-du_x, du_y, -du_φ).
        let mut du = DVector::zeros(n);
        for i in 1..upper_nodes.len() {
            // Pick arbitrary deterministic values
            let dux = 0.001 + 0.0003 * (i as f64);
            let duy = -0.002 + 0.0001 * (i as f64);
            let duφ = 0.005 - 0.0002 * (i as f64);
            // upper
            let nu = upper_nodes[i];
            du[nu.x().index] = dux;
            du[nu.y().index] = duy;
            du[nu.φ().index] = duφ;
            // lower (mirror)
            let nl = lower_nodes[i];
            du[nl.x().index] = -dux;
            du[nl.y().index] = duy;
            du[nl.φ().index] = -duφ;
        }

        // Apply perturbation, recompute internal forces.
        system.set_displacements(&du);
        let mut q2 = DVector::zeros(n);
        system.compute_internal_forces(Some(&mut q2), None, None);

        // Check that q2 is mirror-symmetric: q2[upper.x] = -q2[lower.x], etc.
        for i in 1..upper_nodes.len() {
            let nu = upper_nodes[i];
            let nl = lower_nodes[i];
            let qux = q2[nu.x().index];
            let quy = q2[nu.y().index];
            let quφ = q2[nu.φ().index];
            let qlx = q2[nl.x().index];
            let qly = q2[nl.y().index];
            let qlφ = q2[nl.φ().index];
            // Mirror: forces transform as displacements: x→-x, y→y, φ→-φ.
            let max = qux.abs().max(quy.abs()).max(quφ.abs()).max(1.0);
            assert!((qux + qlx).abs() < 1e-7 * max,
                "node {}: q.x asymmetry: upper={}, lower={}", i, qux, qlx);
            assert!((quy - qly).abs() < 1e-7 * max,
                "node {}: q.y asymmetry: upper={}, lower={}", i, quy, qly);
            assert!((quφ + qlφ).abs() < 1e-7 * max,
                "node {}: q.φ asymmetry: upper={}, lower={}", i, quφ, qlφ);
        }
    }

    /// Verify that adding two mirror-symmetric StringElements (one per limb,
    /// sharing a nock node) preserves mirror symmetry of the global
    /// internal force vector.
    #[test]
    fn test_full_system_with_strings_mirror() {
        use nalgebra::{DMatrix, DVector, vector};
        use virtualbow_num::fem::elements::beam::beam::BeamElement;
        use virtualbow_num::fem::elements::string::StringElement;
        use virtualbow_num::fem::system::dof::DofType;
        use virtualbow_num::fem::system::system::System;
        use virtualbow_num::fem::system::node::Node;

        // Use the ACTUAL flatbow file to reproduce the simulation bug.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data/examples/flatbow.bow");
        let input = BowModel::load(&path).expect("Failed to load flatbow");

        let geom = BowGeometry::new(&input).unwrap()
            .discretize(input.settings.num_limb_eval_points, input.settings.num_limb_elements);
        let brace_pos = geom.draw.brace_pos;

        // Sanity check: upper and lower should be perfect mirrors at the geometry level.
        assert_eq!(geom.upper.p_nodes.len(), geom.lower.p_nodes.len());
        for (i, (pu, pl)) in geom.upper.p_nodes.iter().zip(geom.lower.p_nodes.iter()).enumerate() {
            assert!((pu[0] + pl[0]).abs() < 1e-12,
                "node {}: x asymmetry: upper={}, lower={}", i, pu[0], pl[0]);
            assert!((pu[1] - pl[1]).abs() < 1e-12,
                "node {}: y asymmetry: upper={}, lower={}", i, pu[1], pl[1]);
        }
        for (i, (yu, yl)) in geom.upper.y_nodes.iter().zip(geom.lower.y_nodes.iter()).enumerate() {
            assert!((yu - yl).norm() < 1e-12,
                "node {}: y_nodes asymmetry", i);
        }
        // Verify segment K matrices follow mirror similarity.
        let mut t6 = nalgebra::SMatrix::<f64, 6, 6>::zeros();
        for (i, s) in [-1.0, 1.0, -1.0, -1.0, 1.0, -1.0].iter().enumerate() {
            t6[(i, i)] = *s;
        }
        for (i, (su, sl)) in geom.upper.segments.iter().zip(geom.lower.segments.iter()).enumerate() {
            let expected = t6 * su.K * t6;
            let diff = sl.K - expected;
            let max_err = diff.iter().fold(0.0_f64, |a, &b| a.max(b.abs()));
            let max_val = su.K.iter().fold(0.0_f64, |a, &b| a.max(b.abs()));
            assert!(max_err < 1e-6 * max_val.max(1.0),
                "seg {} K mirror asymmetry: max_err = {}, |K|_max = {}", i, max_err, max_val);
        }

        let mut system = System::new();

        let upper_nodes: Vec<Node> = geom.upper.p_nodes.iter().enumerate().map(|(i, p)| {
            system.create_node(p, &[DofType::active_if(i != 0); 3])
        }).collect();
        let lower_nodes: Vec<Node> = geom.lower.p_nodes.iter().enumerate().map(|(i, p)| {
            system.create_node(p, &[DofType::active_if(i != 0); 3])
        }).collect();

        for (i, seg) in geom.upper.segments.iter().enumerate() {
            system.add_element(&[upper_nodes[i], upper_nodes[i + 1]], BeamElement::new(seg));
        }
        for (i, seg) in geom.lower.segments.iter().enumerate() {
            system.add_element(&[lower_nodes[i], lower_nodes[i + 1]], BeamElement::new(seg));
        }

        // Nock node at (0, brace_pos): x locked, y active, φ locked.
        let nock = system.create_node(
            &vector![0.0, brace_pos, 0.0],
            &[DofType::Locked, DofType::Active, DofType::Locked],
        );

        // Upper string: [nock, upper.nodes[1..]]
        let mut upper_string = vec![nock];
        upper_string.extend(upper_nodes.iter().skip(1).copied());
        let mut offsets_upper = vec![0.0];
        offsets_upper.extend(geom.upper.y_nodes.iter().skip(1).map(|y| y[y.len() - 1]));

        // Lower string: [lower.nodes[1..].rev(), nock]
        let mut lower_string: Vec<Node> = lower_nodes.iter().skip(1).rev().copied().collect();
        lower_string.push(nock);
        let mut offsets_lower: Vec<f64> = geom.lower.y_nodes.iter().skip(1).rev()
            .map(|y| -y[y.len() - 1]).collect();
        offsets_lower.push(0.0);

        let ea = 1e5;
        let s_up = system.add_element(&upper_string, StringElement::new(ea, 0.0, 1.0, 1.0, offsets_upper));
        let s_lo = system.add_element(&lower_string, StringElement::new(ea, 0.0, 1.0, 1.0, offsets_lower));

        // Set unstressed length to current geometric length so initial tension is zero.
        system.update_element(s_up);
        system.update_element(s_lo);
        let l0_u = system.element_ref::<StringElement>(s_up).get_current_length();
        let l0_l = system.element_ref::<StringElement>(s_lo).get_current_length();
        // SHRINK to introduce tension (factor 0.95)
        system.element_mut::<StringElement>(s_up).set_initial_length(0.95 * l0_u);
        system.element_mut::<StringElement>(s_lo).set_initial_length(0.95 * l0_l);

        let n = system.n_dofs();

        // Build a mirror-symmetric perturbation: for each upper.node[i] (active)
        // and corresponding lower.node[i], use (du_x, du_y, du_φ) and
        // (-du_x, du_y, -du_φ). The nock's y-DOF should be a single value
        // (shared between halves naturally).
        let mut du = DVector::zeros(n);
        for i in 1..upper_nodes.len() {
            let dux = 1e-7 + 3e-8 * (i as f64);
            let duy = -2e-7 + 1e-8 * (i as f64);
            let duφ = 5e-7 - 2e-8 * (i as f64);
            let nu = upper_nodes[i]; let nl = lower_nodes[i];
            du[nu.x().index] = dux; du[nu.y().index] = duy; du[nu.φ().index] = duφ;
            du[nl.x().index] = -dux; du[nl.y().index] = duy; du[nl.φ().index] = -duφ;
        }
        // Nock displacement (only y-DOF is active): symmetric perturbation.
        du[nock.y().index] = -0.003;

        system.set_displacements(&du);
        let mut q = DVector::zeros(n);
        let mut k = DMatrix::zeros(n, n);
        system.compute_internal_forces(Some(&mut q), Some(&mut k), None);

        // String tensions should be equal under mirror.
        let n_up = system.element_ref::<StringElement>(s_up).normal_force_total();
        let n_lo = system.element_ref::<StringElement>(s_lo).normal_force_total();
        eprintln!("String tensions: upper = {}, lower = {}", n_up, n_lo);
        assert!((n_up - n_lo).abs() < 1e-6 * n_up.abs().max(1.0),
            "string tensions differ: upper = {}, lower = {}", n_up, n_lo);

        for i in 1..upper_nodes.len() {
            let nu = upper_nodes[i]; let nl = lower_nodes[i];
            let qux = q[nu.x().index]; let quy = q[nu.y().index]; let quφ = q[nu.φ().index];
            let qlx = q[nl.x().index]; let qly = q[nl.y().index]; let qlφ = q[nl.φ().index];
            let max = qux.abs().max(quy.abs()).max(quφ.abs()).max(1.0);
            assert!((qux + qlx).abs() < 1e-6 * max,
                "node {}: q.x asymmetry: upper={}, lower={}", i, qux, qlx);
            assert!((quy - qly).abs() < 1e-6 * max,
                "node {}: q.y asymmetry: upper={}, lower={}", i, quy, qly);
            assert!((quφ + qlφ).abs() < 1e-6 * max,
                "node {}: q.φ asymmetry: upper={}, lower={}", i, quφ, qlφ);
        }

        // Now solve a real static equilibrium with displacement control on the
        // nock_y (mirror-symmetric loading), and verify the converged solution
        // is itself mirror-symmetric.
        use std::f64::consts::FRAC_PI_2;
        use virtualbow_num::fem::solvers::statics::{DisplacementControl, StaticTolerances};
        use virtualbow_num::utils::newton::NewtonSettings;

        // Reset displacements to zero to start solve from symmetric state.
        system.set_displacements(&DVector::zeros(n));
        // Apply unit force on nock_y (mirror-symmetric since nock is on x=0).
        system.add_force(nock.y(), move |_t| -1.0);

        let s_total = *geom.upper.s_eval.last().unwrap();
        let tol = StaticTolerances::new(s_total, FRAC_PI_2, 1e-9);
        let nset = NewtonSettings::default();
        let solver = DisplacementControl::new(&mut system, tol, nset);
        solver.solve_equilibrium(nock.y(), 0.0).expect("static solve failed");

        // Verify the solved displacements are mirror-symmetric.
        let u = system.get_displacements().clone();
        for i in 1..upper_nodes.len() {
            let nu = upper_nodes[i]; let nl = lower_nodes[i];
            let dux = u[nu.x().index]; let duy = u[nu.y().index]; let duφ = u[nu.φ().index];
            let dlx = u[nl.x().index]; let dly = u[nl.y().index]; let dlφ = u[nl.φ().index];
            let max = dux.abs().max(duy.abs()).max(duφ.abs()).max(1e-6);
            eprintln!("node {}: upper=({:.6e},{:.6e},{:.6e}) lower=({:.6e},{:.6e},{:.6e})",
                i, dux, duy, duφ, dlx, dly, dlφ);
            assert!((dux + dlx).abs() < 1e-6 * max,
                "after solve: node {}: u.x asymmetry: upper={:e}, lower={:e}", i, dux, dlx);
            assert!((duy - dly).abs() < 1e-6 * max,
                "after solve: node {}: u.y asymmetry: upper={:e}, lower={:e}", i, duy, dly);
            assert!((duφ + dlφ).abs() < 1e-6 * max,
                "after solve: node {}: u.φ asymmetry: upper={:e}, lower={:e}", i, duφ, dlφ);
        }
        let n_up_post = system.element_ref::<StringElement>(s_up).normal_force_total();
        let n_lo_post = system.element_ref::<StringElement>(s_lo).normal_force_total();
        eprintln!("Post-solve string tensions: upper = {}, lower = {}", n_up_post, n_lo_post);
        assert!((n_up_post - n_lo_post).abs() < 1e-6 * n_up_post.abs().max(1.0),
            "after solve: string tensions differ: upper={}, lower={}", n_up_post, n_lo_post);
    }
}
