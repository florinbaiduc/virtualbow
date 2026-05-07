//! Bow simulation core (v0.11+, full asymmetric model).
//!
//! Both limb halves are meshed as independent FEM chains anchored at the grip.
//! The lower limb is mirrored across the y-axis so that its world x-coordinates
//! are negated and its in-plane rotation is reflected as `π - φ`.
//!
//! The string is modelled as one continuous `StringElement` whose contact
//! polyline runs:
//!
//!   `upper_tip → upper_n-1 → … → upper_root → nock → lower_root → … → lower_tip`
//!
//! The nock node is a free 2-DOF point (x locked at `nock_offset` for v1,
//! y free during static draw / dynamic shot).  Bracing is solved by iterating
//! on the string's unstressed length until the upper-side string is tangent to
//! the locked-x direction at the nock — this matches the v4 criterion exactly
//! for symmetric bows.

use std::f64::consts::{PI, FRAC_PI_2};
use clap::ValueEnum;
use itertools::Itertools;
use iter_num_tools::lin_space;
use nalgebra::{SMatrix, SVector, vector};
use virtualbow_num::fem::solvers::eigen::{Mode, natural_frequencies};
use virtualbow_num::fem::solvers::statics::{DisplacementControl, LoadControl, StaticTolerances};
use virtualbow_num::fem::system::element::Element;
use virtualbow_num::fem::system::node::Node;
use virtualbow_num::fem::system::system::{System, SystemEval};
use crate::errors::ModelError;
use crate::geometry::{BowGeometry, DiscreteBowGeometry, DiscreteLimbHalfGeometry};
use crate::input::{ArrowMass, BowModel, Handle, Line, LimbSide, LimbSection, Material, ProfileSegment};
use crate::output::{ArrowDeparture, BowResult, Common, Dynamics, LayerInfo, MaxForces, MaxStresses, State, StateVec, Statics};
use crate::profile::profile::{CurvePoint, ProfileCurve};
use crate::sections::section::LayeredCrossSection;
use virtualbow_num::fem::elements::beam::beam::BeamElement;
use virtualbow_num::fem::elements::beam::geometry::{CrossSection, PlanarCurve};
use virtualbow_num::fem::elements::beam::linear::LinearBeamSegment;
use virtualbow_num::fem::elements::mass::MassElement;
use virtualbow_num::fem::elements::string::StringElement;
use virtualbow_num::fem::solvers::dynamics::{DynamicSolver, DynamicSolverSettings, DynamicTolerances, StopCondition, TimeStepping};
use virtualbow_num::fem::system::dof::DofType;
use virtualbow_num::utils::integration::cumulative_simpson;
use virtualbow_num::utils::roots::find_root_falsi;
use virtualbow_num::utils::minmax::{discrete_maximum_nd, discrete_minimum_nd};
use virtualbow_num::utils::newton::NewtonSettings;

#[derive(ValueEnum, PartialEq, Debug, Copy, Clone)]
pub enum SimulationMode {
    Static,
    Dynamic
}

/// Static eval data for a `Handle::Beam` grip half, used to render the
/// grip outline in the result viewer. Mirrors a subset of `LimbInfo`.
#[derive(Default, Clone)]
struct GripEvalInfo {
    length: Vec<f64>,                              // s along grip arc
    width: Vec<f64>,                               // section width
    height: Vec<f64>,                              // total section height
    bounds: Vec<Vec<f64>>,                         // layer boundaries
    ratio: Vec<f64>,                               // normalised s
    heights: Vec<Vec<f64>>,                        // per-layer heights
    position_eval: Vec<SVector<f64, 3>>,           // world-frame (x, y, φ)
    position_control: Vec<SVector<f64, 3>>,        // start/end nodes
    curvature_eval: Vec<f64>,                      // 0 for the Line grip
}

/// FEM-side handles for one limb chain.
struct LimbChain {
    nodes: Vec<Node>,                  // root → … → tip
    elements: Vec<usize>,              // beam element ids
    /// Grip beam element ids for `Handle::Beam`, in order from the pivot
    /// (clamped) to the joint (shared with the limb root). Empty for
    /// `Handle::Rigid` and `Handle::Flexible`. Contributing to evaluation
    /// produces the grip's static and per-state positions/strains/forces
    /// alongside the limb's, so the result viewer renders one continuous
    /// outline through the grip.
    grip_elements: Vec<usize>,
    /// Grip static eval data (positions/bounds/etc.) to be prepended to
    /// the limb's `LimbInfo`. None for non-Beam handles.
    grip_eval: Option<GripEvalInfo>,
    mass_element_tip: usize,           // limb-tip mass element
    mass_element_string_tip: usize,    // string-tip mass element (added later)
}

pub struct Simulation<'a> {
    input: &'a BowModel,
    geometry: DiscreteBowGeometry,

    upper: LimbChain,
    lower: LimbChain,

    nock_node: Node,
    string_element_upper: usize,
    string_element_lower: usize,

    mass_element_arrow: usize,         // arrow mass at the nock
    mass_element_nock: usize,          // nock-point fixed mass

    arrow_mass: f64,
    arrow_departure: Option<(usize, f64, f64, f64)>,
}

/// (No world-frame transform is applied here: lower-limb geometry is built
/// directly in world coordinates by the geometry module via a mirrored
/// curve adapter.)

impl<'a> Simulation<'a> {
    // Numerical constants for the bracing simulation
    const BRACING_DELTA_START: f64 = 1e-3;
    const BRACING_DELTA_MIN: f64 = 1e-5;
    const BRACING_SLOPE_TOL: f64 = 1e-6;
    const BRACING_MAX_ROOT_ITER: usize = 20;
    const BRACING_TARGET_ITER: usize = 5;

    // Numerical constants for the asymmetric refinement (2D Newton on the
    // independent unstressed-length factors of the upper and lower string
    // halves).  For symmetric bows this stage converges immediately.
    const ASYM_BRACING_MAX_ITER: usize = 10;
    const ASYM_BRACING_FD_STEP: f64 = 1.0e-5;
    const ASYM_BRACING_R1_TOL: f64 = 1.0e-7;    // collinearity metric
    const ASYM_BRACING_R2_TOL: f64 = 1.0e-3;    // nock reaction (Newtons)
    const ASYM_BRACING_MAX_STEP: f64 = 0.01;

    fn build_chain(
        system: &mut System,
        _side: LimbSide,
        half: &DiscreteLimbHalfGeometry,
        limb_tip_mass: f64,
        existing_root: Option<Node>,
    ) -> LimbChain {
        // Place limb nodes at their geometric (world-frame) positions.
        // If `existing_root` is provided (e.g. from a Beam handle's grip
        // chain), use it as nodes[0] without creating a new node — the
        // root inherits its DOF state from the grip (active, since the
        // pivot end of the grip is what's locked, not the joint end).
        // Otherwise the root is fully fixed (rigid handle / flexible joint).
        let nodes: Vec<Node> = half.p_nodes.iter().enumerate().map(|(i, p_world)| {
            if i == 0 {
                if let Some(root) = existing_root {
                    return root;
                }
            }
            system.create_node(p_world, &[DofType::active_if(i != 0); 3])
        }).collect();

        let elements: Vec<usize> = half.segments.iter().enumerate().map(|(i, segment)| {
            system.add_element(&[nodes[i], nodes[i+1]], BeamElement::new(segment))
        }).collect();

        // Limb-tip mass element (required for limb damping calibration).
        let mass_element_tip = system.add_element(
            &[*nodes.last().unwrap()],
            MassElement::point(limb_tip_mass),
        );

        // String-tip mass: zero initially, set after the bracing simulation
        // determines the string's actual length.
        let mass_element_string_tip = system.add_element(
            &[*nodes.last().unwrap()],
            MassElement::point(0.0),
        );

        LimbChain { nodes, elements, grip_elements: Vec::new(), grip_eval: None, mass_element_tip, mass_element_string_tip }
    }

    /// Build a grip beam chain for one side of a `Handle::Beam`.
    ///
    /// The grip is meshed as a horizontal Line of length `grip_length` with
    /// the supplied cross-section. The pivot end (s = 0) is fully clamped;
    /// the joint end (s = grip_length) is returned as an active node so the
    /// caller can hand it to `build_chain` as the limb's root.
    ///
    /// Reference frame: identical to the limb-half local frame (extends +x
    /// from the pivot toward the joint). For the lower side the segments
    /// and node positions are mirrored to world coordinates exactly as
    /// `geometry::mirror_discrete_to_world` does for limbs, so reference
    /// angles agree between the grip's joint node and the lower limb's
    /// root node.
    ///
    /// Limitation: the grip is built with constant local tangent angle 0;
    /// `BeamHandle.angle` is reflected only in the limb-positioning logic
    /// (via `to_rigid()`), not in the grip path itself. For a yumi
    /// (`angle = 0`) this is exact.
    fn build_grip_chain(
        system: &mut System,
        side: LimbSide,
        grip_length: f64,
        grip_section_input: &LimbSection,
        materials: &[Material],
        n_elements: usize,
    ) -> Result<(Vec<Node>, Vec<usize>, Node, GripEvalInfo), ModelError> {
        // Local-frame Line profile: from (0,0,0) along +x for `grip_length`.
        let start = CurvePoint::new(0.0, 0.0, vector![0.0, 0.0]);
        let curve = ProfileCurve::new(start, &[ProfileSegment::Line(Line::new(grip_length))])?;
        let section = LayeredCrossSection::new(grip_section_input, materials)?;

        // Discretise into `n_elements` LinearBeamSegments. We use 3 eval
        // points per segment (start, middle, end) and de-duplicate at
        // interior nodes (mirrors `LimbHalfGeometry::discretize_with`).
        let s_node: Vec<f64> = lin_space(curve.length_start()..=curve.length_end(), n_elements + 1).collect();
        let mut segments: Vec<LinearBeamSegment> = s_node.iter().tuple_windows().enumerate().map(|(i, (&s0, &s1))| {
            let s_eval: Vec<f64> = if i == 0 {
                vec![s0, 0.5*(s0 + s1), s1]
            } else {
                vec![0.5*(s0 + s1), s1]
            };
            LinearBeamSegment::new(&curve, &section, s0, s1, &s_eval)
        }).collect();

        // Local-frame node positions (x, y, φ) along the Line.
        let mut p_nodes: Vec<SVector<f64, 3>> = s_node.iter().map(|&s| {
            let r = curve.position(s);
            let phi = curve.angle(s);
            vector![r[0], r[1], phi]
        }).collect();

        // Build a parallel list of grip eval s-values for the result viewer
        // (matches the per-segment s_eval choices above, de-duplicating at
        // interior nodes).
        let mut s_eval_all: Vec<f64> = Vec::new();
        for (i, (&s0, &s1)) in s_node.iter().tuple_windows().enumerate() {
            if i == 0 { s_eval_all.push(s0); }
            s_eval_all.push(0.5*(s0 + s1));
            s_eval_all.push(s1);
        }
        // Continuous-geometry quantities at the eval points (in LOCAL frame).
        let n_eval: Vec<f64> = s_eval_all.iter().map(|&s| curve.normalize(s)).collect();
        let mut p_eval: Vec<SVector<f64, 3>> = s_eval_all.iter().map(|&s| {
            let r = curve.position(s);
            let phi = curve.angle(s);
            vector![r[0], r[1], phi]
        }).collect();
        let bounds: Vec<Vec<f64>> = n_eval.iter().map(|&n| section.layer_bounds(n).0.data.clone().into()).collect();
        let heights: Vec<Vec<f64>> = n_eval.iter().map(|&n| section.layer_bounds(n).1.data.clone().into()).collect();
        let widths: Vec<f64> = n_eval.iter().map(|&n| section.width(n)).collect();
        let total_heights: Vec<f64> = heights.iter().map(|h| h.iter().sum::<f64>()).collect();
        let curvature_eval: Vec<f64> = vec![0.0; s_eval_all.len()];
        let mut p_control: Vec<SVector<f64, 3>> = s_node.iter().map(|&s| {
            let r = curve.position(s);
            let phi = curve.angle(s);
            vector![r[0], r[1], phi]
        }).collect();

        // For the lower side, mirror everything into world frame using the
        // same transform as `geometry::mirror_discrete_to_world`:
        //   (x, y, φ) → (-x, y, π - φ) on positions
        //   K → T6 · K · T6 with T6 = diag(-1, 1, -1, -1, 1, -1) on stiffness
        if side == LimbSide::Lower {
            let mirror_p3 = |p: &SVector<f64, 3>| vector![-p[0], p[1], PI - p[2]];
            for p in p_nodes.iter_mut()    { *p = mirror_p3(p); }
            for p in p_eval.iter_mut()     { *p = mirror_p3(p); }
            for p in p_control.iter_mut()  { *p = mirror_p3(p); }
            let mut t6 = SMatrix::<f64, 6, 6>::zeros();
            for (i, s) in [-1.0, 1.0, -1.0, -1.0, 1.0, -1.0].iter().enumerate() { t6[(i, i)] = *s; }
            let mut t3 = SMatrix::<f64, 3, 3>::zeros();
            for (i, s) in [-1.0, 1.0, -1.0].iter().enumerate() { t3[(i, i)] = *s; }
            for seg in segments.iter_mut() {
                seg.p0 = mirror_p3(&seg.p0);
                seg.p1 = mirror_p3(&seg.p1);
                for p in seg.pe.iter_mut() { *p = mirror_p3(p); }
                seg.K = t6 * seg.K * t6;
                for e in seg.Ep.iter_mut() { *e = t3 * (*e) * t6; }
                for e in seg.Ef.iter_mut() { *e = t3 * (*e) * t6; }
            }
        }

        // Create FEM nodes. Pivot (index 0) is fully locked; joint (last)
        // and intermediate nodes are active.
        let nodes: Vec<Node> = p_nodes.iter().enumerate().map(|(i, p)| {
            system.create_node(p, &[DofType::active_if(i != 0); 3])
        }).collect();

        let elements: Vec<usize> = segments.iter().enumerate().map(|(i, segment)| {
            system.add_element(&[nodes[i], nodes[i+1]], BeamElement::new(segment))
        }).collect();

        let joint_node = *nodes.last().unwrap();
        let eval = GripEvalInfo {
            length: s_eval_all,
            width: widths,
            height: total_heights,
            bounds,
            ratio: n_eval,
            heights,
            position_eval: p_eval,
            position_control: p_control,
            curvature_eval,
        };
        Ok((nodes, elements, joint_node, eval))
    }

    fn initialize(model: &'a BowModel, string: bool, damping: bool) -> Result<(System, Simulation<'a>, Common), ModelError> {
        model.validate()?;

        // Continuous geometry, then discretized
        let geometry = BowGeometry::new(model)?;
        let geometry = geometry.discretize(model.settings.num_limb_eval_points, model.settings.num_limb_elements);

        // Layer setup data (shared across limbs since materials are bow-global
        // and the layer list comes from the upper limb section — for the
        // result viewer we only carry one set; per-limb result arrays
        // distinguish back/belly stresses themselves).
        let layers = model.section.upper.layers.iter().map(|layer| {
            let material = model.section.materials.iter().find(|mat| mat.name == layer.material).unwrap();
            LayerInfo {
                name: layer.name.clone(),
                color: material.color.clone(),
                maximum_stresses: material.maximum_stresses(),
                allowed_stresses: material.allowed_stresses(),
                maximum_strains: material.maximum_strains(),
                allowed_strains: material.allowed_strains(),
            }
        }).collect_vec();

        let mut system = System::new();

        // If the user picked Handle::Beam, build the two grip cantilevers
        // first so we can hand their joint-end nodes to `build_chain` as
        // the limb roots. Each grip half is locked at its pivot end.
        // For Rigid / Flexible handles, no grip elements are created and the
        // limbs are clamped at their roots as before.
        let (grip_upper_root, grip_lower_root, grip_elements_upper, grip_elements_lower, grip_eval_upper, grip_eval_lower) = match &model.handle {
            Handle::Beam(b) => {
                let (_g_nodes_u, g_elems_u, joint_u, eval_u) = Self::build_grip_chain(
                    &mut system, LimbSide::Upper, b.length_upper, &b.section,
                    &model.section.materials, b.n_elements_upper,
                )?;
                let (_g_nodes_l, g_elems_l, joint_l, eval_l) = Self::build_grip_chain(
                    &mut system, LimbSide::Lower, b.length_lower, &b.section,
                    &model.section.materials, b.n_elements_lower,
                )?;
                (Some(joint_u), Some(joint_l), g_elems_u, g_elems_l, Some(eval_u), Some(eval_l))
            }
            _ => (None, None, Vec::new(), Vec::new(), None, None),
        };

        let mut upper = Self::build_chain(&mut system, LimbSide::Upper, &geometry.upper, model.masses.limb_tip_upper, grip_upper_root);
        let mut lower = Self::build_chain(&mut system, LimbSide::Lower, &geometry.lower, model.masses.limb_tip_lower, grip_lower_root);
        upper.grip_elements = grip_elements_upper;
        upper.grip_eval = grip_eval_upper;
        lower.grip_elements = grip_elements_lower;
        lower.grip_eval = grip_eval_lower;

        // Limb damping (per limb, but using the same target ratio for both).
        // Grip elements receive the same alpha so their viscous damping is
        // calibrated against the bow's first natural frequency too.
        if damping && model.damping.damping_ratio_limbs != 0.0 {
            let modes = natural_frequencies(&mut system).map_err(ModelError::SimulationEigenSolutionFailed)?;
            let alpha = 2.0*model.damping.damping_ratio_limbs/modes[0].omega;
            for &e in upper.elements.iter().chain(lower.elements.iter()).chain(upper.grip_elements.iter()).chain(lower.grip_elements.iter()) {
                system.element_mut::<BeamElement>(e).set_damping(alpha);
            }
        }

        // Nock node: x locked at nock_offset, y active (or locked when string disabled).
        let nock_node = system.create_node(
            &vector![geometry.draw.nock_offset, geometry.draw.brace_pos, 0.0],
            &[DofType::Locked, DofType::active_if(string), DofType::Locked],
        );

        // Build TWO independent string elements, one per limb half, sharing
        // the nock as their boundary node.  The orientation of each element's
        // node list matters: the StringElement performs convex-envelope
        // contact detection with a fixed `LeftTurn` (CCW) orientation, which
        // requires the polyline to traverse +x while wrapping over the +y
        // (back-of-bow) side.  We therefore use:
        //   upper element: [nock, upper.nodes[1], …, upper_tip]   (runs +x)
        //   lower element: [lower_tip, …, lower.nodes[1], nock]   (runs +x)
        // This matches the v4 topology robustness.  Using two elements with
        // the same axial stiffness EA is mechanically equivalent to one
        // continuous string of total length L_upper + L_lower.
        let n_active_upper = upper.nodes.len() - 1;     // active = all except root
        let n_active_lower = lower.nodes.len() - 1;
        let mut upper_string_nodes = Vec::<Node>::with_capacity(n_active_upper + 1);
        upper_string_nodes.push(nock_node);
        upper_string_nodes.extend(upper.nodes.iter().skip(1).copied());
        let mut lower_string_nodes = Vec::<Node>::with_capacity(n_active_lower + 1);
        lower_string_nodes.extend(lower.nodes.iter().skip(1).rev().copied());
        lower_string_nodes.push(nock_node);

        // Layered offsets at every contact point, matching node order.
        // The StringElement places surface points at
        //     (x_surface, y_surface) = (x - offset·sin φ, y + offset·cos φ)
        // For the upper limb the world-frame angle is φ ≈ 0, so positive
        // offsets place the surface on the +y (back-of-bow) side.  For the
        // lower limb the world-frame angle is φ ≈ π, so cos φ = -1; we
        // therefore negate the offsets so the surface is still on +y.
        let mut offsets_upper = Vec::<f64>::with_capacity(upper_string_nodes.len());
        offsets_upper.push(0.0);
        for y in geometry.upper.y_nodes.iter().skip(1) { offsets_upper.push(y[y.len() - 1]); }
        // Lower element node order is [lower_tip, …, lower.nodes[1], nock] —
        // i.e. y_nodes is consumed in reverse from `tip..=1`, then 0 for nock.
        let mut offsets_lower = Vec::<f64>::with_capacity(lower_string_nodes.len());
        for y in geometry.lower.y_nodes.iter().skip(1).rev() { offsets_lower.push(-y[y.len() - 1]); }
        offsets_lower.push(0.0);

        // Mass elements on the nock (arrow mass and a fixed nock mass).
        let mass_element_arrow = system.add_element(&[nock_node], MassElement::point(0.0));
        let mass_element_nock = system.add_element(&[nock_node], MassElement::point(0.0));

        // String element parameters.  Damping is set later, once we know the
        // unstressed string length; compression factor is set in dynamic mode.
        let EA = if string { (model.string.n_strands as f64)*model.string.strand_stiffness } else { 0.0 };
        let ρA = if string { (model.string.n_strands as f64)*model.string.strand_density } else { 0.0 };

        let string_element_upper = system.add_element(&upper_string_nodes, StringElement::new(EA, 0.0, 1.0, 1.0, offsets_upper));
        let string_element_lower = system.add_element(&lower_string_nodes, StringElement::new(EA, 0.0, 1.0, 1.0, offsets_lower));

        // Set the unstressed length of each half to its current geometric
        // length so the string is initially tension-free.
        system.update_element(string_element_upper);
        system.update_element(string_element_lower);
        let unstressed_length_upper = system.element_ref::<StringElement>(string_element_upper).get_current_length();
        let unstressed_length_lower = system.element_ref::<StringElement>(string_element_lower).get_current_length();
        system.element_mut::<StringElement>(string_element_upper).set_initial_length(unstressed_length_upper);
        system.element_mut::<StringElement>(string_element_lower).set_initial_length(unstressed_length_lower);

        // ----- Bracing simulation (only if string is enabled) -----
        if string {
            // Bracing criterion: the string must run STRAIGHT through the
            // nock at brace, i.e. the nock must lie on the line connecting
            // its two adjacent string-contact points (one on each limb).
            // For a symmetric bow this reduces to the classic "upper-side
            // string is horizontal at the nock" condition (slope = 0).  For
            // an asymmetric bow (e.g. yumi), upper- and lower-side string
            // segments leave the nock at different angles; requiring the
            // string to be collinear at the nock is the natural extension
            // and produces a visually correct brace where the string is
            // not artificially V-bent at the nock — without this, frame 0
            // of the static simulation looks pre-drawn.
            //
            // The metric is the signed cross product of the two segments
            // emanating from the nock; it is zero when they are collinear
            // (i.e. point in opposite directions).  For the upper string
            // element, contact 0 is the nock and contact 1 is the next
            // contact toward the upper limb tip.  For the lower string
            // element (whose nodes run [lower_tip, …, nock]), the nock is
            // the LAST contact and the previous one is the next contact
            // toward the lower limb tip.
            let get_collinearity_metric = |system: &System| -> f64 {
                let elem_u = system.element_ref::<StringElement>(string_element_upper);
                let mut it_u = elem_u.contact_positions();
                let p_n = it_u.next().expect("at least two upper string contacts");
                let p_u = it_u.next().expect("at least two upper string contacts");

                let elem_l = system.element_ref::<StringElement>(string_element_lower);
                let mut p_l_prev: Option<SVector<f64, 2>> = None;
                let mut p_l_last: Option<SVector<f64, 2>> = None;
                for p in elem_l.contact_positions() {
                    p_l_prev = p_l_last;
                    p_l_last = Some(p);
                }
                let p_l = p_l_prev.expect("at least two lower string contacts");

                // Signed cross product of (p_l - p_n) × (p_u - p_n).
                // Zero ⇔ p_u, p_n, p_l are collinear AND p_n lies on the
                // segment between them (string passes straight through).
                // We normalise by (p_u.x - p_n.x) so the metric reduces
                // exactly to the previous "upper slope" expression for a
                // symmetric bow (where p_l is the mirror of p_u and the
                // metric is 2·slope_upper·(p_u.x - p_n.x), preserving the
                // sign of the original criterion: positive at the unbraced
                // start, decreasing through zero at brace).
                let dx_u = p_u[0] - p_n[0];
                let dy_u = p_u[1] - p_n[1];
                let dx_l = p_l[0] - p_n[0];
                let dy_l = p_l[1] - p_n[1];
                (dy_l*dx_u - dx_l*dy_u)/dx_u
            };

            // Apply a unit downward force on the nock so DisplacementControl has
            // a reference direction.  This also gives the static-draw solver
            // a non-zero load vector to scale via its load factor.
            system.add_force(nock_node.y(), move |_t| -1.0);

            let mut factor1 = 1.0;
            let mut slope1 = get_collinearity_metric(&system);
            let mut delta = Self::BRACING_DELTA_START;

            if slope1 < 0.0 {
                return Err(ModelError::SimulationBraceHeightTooLow(model.draw.brace_height));
            }

            let s_eval_total = (*geometry.upper.s_eval.last().unwrap()).max(*geometry.lower.s_eval.last().unwrap());

            let mut try_string_length = |factor: f64| {
                system.element_mut::<StringElement>(string_element_upper).set_initial_length(factor*unstressed_length_upper);
                system.element_mut::<StringElement>(string_element_lower).set_initial_length(factor*unstressed_length_lower);
                let tolerances = StaticTolerances::new(s_eval_total, FRAC_PI_2, model.settings.static_iteration_tolerance);
                let settings = NewtonSettings::default();
                let solver = DisplacementControl::new(&mut system, tolerances, settings);
                let result = solver.solve_equilibrium(nock_node.y(), 0.0);
                let slope = get_collinearity_metric(&system);
                (slope, result)
            };

            loop {
                let factor2 = factor1 - delta;
                let (slope2, result) = try_string_length(factor2);
                if let Ok(info) = result {
                    if slope2 <= 0.0 {
                        let try_string_length_for_root = |factor| try_string_length(factor).0;
                        match find_root_falsi(try_string_length_for_root, factor1, factor2, slope1, slope2, 0.0, Self::BRACING_SLOPE_TOL, Self::BRACING_MAX_ROOT_ITER) {
                            Some(_) => break,
                            None => {
                                // Root finder failed — slope is too non-linear in this
                                // interval.  Halve delta and retry from factor1.
                                delta /= 4.0;
                            }
                        }
                    } else {
                        factor1 = factor2;
                        slope1 = slope2;
                        // Adapt step size, but cap growth to 2× per step.
                        let scale = (Self::BRACING_TARGET_ITER as f64) / (info.iterations as f64);
                        delta *= scale.min(2.0);
                    }
                } else {
                    delta /= 2.0;
                }

                if delta < Self::BRACING_DELTA_MIN {
                    return Err(ModelError::SimulationBracingNoSignChange);
                }
            }

            // ----- Asymmetric refinement -----
            //
            // Real-world bows have ONE continuous string knotted at the nock.
            // The knot does not slip, so the upper and lower halves can carry
            // different tensions, but the nock itself must be in self-
            // equilibrium at brace (the archer's hand applies no force).
            //
            // The 1D bracing above only enforces collinearity of the string
            // at the nock — it tunes one scalar `factor` shared by both
            // halves.  For a SYMMETRIC bow the y-reaction at the nock is
            // zero by symmetry once collinearity holds, so we are done.
            // For an ASYMMETRIC bow (e.g. yumi) the two halves still carry
            // unequal tensions in this state, leaving a residual y-force at
            // the nock that shows up as a non-zero `draw_force[0]` in the
            // static result.
            //
            // We refine here with a 2D Newton iteration on (f_u, f_l) — the
            // unstressed-length factors of the two halves, taken
            // independently — driving BOTH the collinearity metric AND the
            // nock's constraint reaction (the Lagrange multiplier λ from
            // DisplacementControl, which equals the y-force the string must
            // exert on the nock to keep it pinned at brace_pos) to zero.
            //
            // Starting point: f_u = f_l = (current scalar factor).  For a
            // symmetric bow the iteration converges in one step (residuals
            // already at machine precision).  For an asymmetric bow, the
            // halves drift apart by a small fraction of a percent — within
            // the latitude any real bowyer has when tying off the string.
            let f0 = system.element_ref::<StringElement>(string_element_upper).get_initial_length()/unstressed_length_upper;
            let mut f_u = f0;
            let mut f_l = f0;

            let mut try_lengths = |f_u: f64, f_l: f64| -> (f64, f64, bool) {
                system.element_mut::<StringElement>(string_element_upper).set_initial_length(f_u*unstressed_length_upper);
                system.element_mut::<StringElement>(string_element_lower).set_initial_length(f_l*unstressed_length_lower);
                let tolerances = StaticTolerances::new(s_eval_total, FRAC_PI_2, model.settings.static_iteration_tolerance);
                let settings = NewtonSettings::default();
                let solver = DisplacementControl::new(&mut system, tolerances, settings);
                match solver.solve_equilibrium(nock_node.y(), 0.0) {
                    Ok(info) => {
                        let r1 = get_collinearity_metric(&system);
                        // λ is the load-factor multiplier; the applied
                        // reference force at nock_y was -1 N, so the
                        // string's net y-pull on the nock is -λ.  Driving
                        // λ → 0 gives a freely-supported nock at brace.
                        let r2 = info.λ;
                        (r1, r2, true)
                    }
                    Err(_) => (0.0, 0.0, false),
                }
            };

            for iter in 0..Self::ASYM_BRACING_MAX_ITER {
                let (r1, r2, ok) = try_lengths(f_u, f_l);
                if !ok { break; }
                if r1.abs() < Self::ASYM_BRACING_R1_TOL && r2.abs() < Self::ASYM_BRACING_R2_TOL {
                    break;
                }
                if iter == Self::ASYM_BRACING_MAX_ITER - 1 { break; }

                // Finite-difference Jacobian.
                let h = Self::ASYM_BRACING_FD_STEP;
                let (r1_du, r2_du, ok_u) = try_lengths(f_u + h, f_l);
                if !ok_u { break; }
                let (r1_dl, r2_dl, ok_l) = try_lengths(f_u, f_l + h);
                if !ok_l { break; }
                let j11 = (r1_du - r1)/h;
                let j21 = (r2_du - r2)/h;
                let j12 = (r1_dl - r1)/h;
                let j22 = (r2_dl - r2)/h;

                let det = j11*j22 - j12*j21;
                if det.abs() < 1e-30 { break; }
                let mut df_u = -(j22*r1 - j12*r2)/det;
                let mut df_l = -(-j21*r1 + j11*r2)/det;

                // Damp large steps: the fiber strain is ~(f - 1) so a step
                // of 1% in f corresponds to ~1% strain change.
                let step_norm = (df_u*df_u + df_l*df_l).sqrt();
                if step_norm > Self::ASYM_BRACING_MAX_STEP {
                    let scale = Self::ASYM_BRACING_MAX_STEP/step_norm;
                    df_u *= scale;
                    df_l *= scale;
                }
                f_u += df_u;
                f_l += df_l;
            }

            // Restore the converged lengths in case the last iteration was
            // a Jacobian probe.
            let _ = try_lengths(f_u, f_l);
        }

        // String damping & string-tip / nock additional masses.
        let l0_upper = system.element_ref::<StringElement>(string_element_upper).get_initial_length();
        let l0_lower = system.element_ref::<StringElement>(string_element_lower).get_initial_length();
        let l0 = l0_upper + l0_lower;
        let ηA = 4.0*l0/PI*f64::sqrt(ρA*EA)*model.damping.damping_ratio_string;
        system.element_mut::<StringElement>(string_element_upper).set_linear_damping(ηA);
        system.element_mut::<StringElement>(string_element_lower).set_linear_damping(ηA);

        // Distribute the linear string mass with a 3-node lump per half:
        // 1/3 to each end — i.e. limb tip and nock — so the nock receives
        // the contributions from BOTH halves (1/3 + 1/3 = 2/3) and each
        // limb tip receives 1/3 of its half.
        let string_linear_mass = ρA*l0;
        let string_linear_mass_upper = ρA*l0_upper;
        let string_linear_mass_lower = ρA*l0_lower;
        system.element_mut::<MassElement>(mass_element_nock).set_mass(model.masses.string_nock + (1.0/3.0)*(string_linear_mass_upper + string_linear_mass_lower));
        system.element_mut::<MassElement>(upper.mass_element_string_tip).set_mass(model.masses.string_tip_upper + (1.0/3.0)*string_linear_mass_upper);
        system.element_mut::<MassElement>(lower.mass_element_string_tip).set_mass(model.masses.string_tip_lower + (1.0/3.0)*string_linear_mass_lower);

        // Geometry summary values for the output Common block.
        let power_stroke = geometry.draw.power_stroke;
        let string_length = l0;                                 // full string length (nock-to-nock in v4 sense was 2*l0; here l0 already spans the whole string)
        let string_stiffness = EA/string_length;
        let string_mass = string_linear_mass + model.masses.string_tip_upper + model.masses.string_tip_lower + model.masses.string_nock;
        let limb_mass_upper = geometry.upper.segments.iter().map(|s| s.m).sum::<f64>() + model.masses.limb_tip_upper;
        let limb_mass_lower = geometry.lower.segments.iter().map(|s| s.m).sum::<f64>() + model.masses.limb_tip_lower;

        let simulation = Self {
            input: model,
            geometry,
            upper,
            lower,
            nock_node,
            string_element_upper,
            string_element_lower,
            mass_element_arrow,
            mass_element_nock,
            arrow_mass: 0.0,
            arrow_departure: None,
        };

        let common = Common {
            limb: prepend_grip_to_limb_info(&simulation.upper.grip_eval, simulation.geometry.upper.to_limb_info()),
            limb_lower: prepend_grip_to_limb_info(&simulation.lower.grip_eval, simulation.geometry.lower.to_limb_info()),
            layers,
            pivot_point: simulation.geometry.draw.pivot_point,
            nock_offset: simulation.geometry.draw.nock_offset,
            power_stroke,
            string_length,
            string_stiffness,
            string_mass,
            limb_mass: limb_mass_upper,
            limb_mass_lower,
        };

        Ok((system, simulation, common))
    }

    pub fn simulate<F>(model: &'a BowModel, mode: SimulationMode, mut callback: F) -> Result<BowResult, ModelError>
    where F: FnMut(SimulationMode, f64) -> bool
    {
        let (mut system, mut simulation, common) = Self::initialize(model, true, mode == SimulationMode::Dynamic)?;

        let statics = {
            let s_eval_total = (*simulation.geometry.upper.s_eval.last().unwrap()).max(*simulation.geometry.lower.s_eval.last().unwrap());
            let tolerances = StaticTolerances::new(s_eval_total, FRAC_PI_2, model.settings.static_iteration_tolerance);
            let settings = NewtonSettings::default();

            let solver = DisplacementControl::new(&mut system, tolerances, settings);
            let mut states = StateVec::new();

            let nock_dof = simulation.nock_node.y();
            solver.solve_equilibrium_path(nock_dof, -simulation.geometry.draw.power_stroke, model.settings.min_draw_resolution, &mut |system, eval, info| {
                let stiffness = 1.0/info.dxdλ[nock_dof.index];
                let state = simulation.get_bow_state(system, eval, -stiffness);
                let progress = state.power_stroke/simulation.geometry.draw.power_stroke;
                states.push(state);
                callback(SimulationMode::Static, 100.0*progress)
            }).map_err(ModelError::SimulationStaticSolutionFailed)?;

            let e_pot_front = states.elastic_energy_limbs.first().unwrap() + states.elastic_energy_string.first().unwrap();
            let e_pot_back = states.elastic_energy_limbs.last().unwrap() + states.elastic_energy_string.last().unwrap();

            let final_draw_force = *states.draw_force.last().unwrap();
            let final_drawing_work = e_pot_back - e_pot_front;
            let storage_factor = (e_pot_back - e_pot_front)/(0.5*simulation.geometry.draw.power_stroke*final_draw_force);

            let max_forces = MaxForces::from_states(&states);
            let max_stresses = MaxStresses::from_states(&states);

            Statics {
                states,
                final_draw_force,
                final_drawing_work,
                storage_factor,
                max_forces,
                max_stresses,
            }
        };

        let dynamics = {
            if mode == SimulationMode::Dynamic {
                simulation.arrow_mass = match model.masses.arrow {
                    ArrowMass::Mass(mass) => mass,
                    ArrowMass::MassPerForce(mass) => mass*statics.final_draw_force,
                    ArrowMass::MassPerEnergy(mass) => mass*statics.final_drawing_work,
                };

                // Full arrow mass (the bow is now modelled in full, no symmetry halving).
                system.element_mut::<MassElement>(simulation.mass_element_arrow).set_mass(simulation.arrow_mass);

                let k_bow = statics.final_draw_force/(model.draw.draw_length.from_pivot() - model.draw.brace_height);
                let t_max = model.settings.timeout_factor*FRAC_PI_2*f64::sqrt(simulation.arrow_mass/k_bow);
                let step = TimeStepping::Adaptive {
                    min_timestep: model.settings.min_timestep,
                    max_timestep: model.settings.max_timestep,
                    steps_per_period: model.settings.steps_per_period,
                };

                let s_eval_total = (*simulation.geometry.upper.s_eval.last().unwrap()).max(*simulation.geometry.lower.s_eval.last().unwrap());
                let ref_linear_acc = statics.final_draw_force/simulation.arrow_mass;
                let ref_angular_acc = ref_linear_acc/s_eval_total;
                let tolerances = DynamicTolerances::new(ref_linear_acc, ref_angular_acc, model.settings.dynamic_iteration_tolerance);
                let settings = DynamicSolverSettings { time_stepping: step, max_time: t_max, newton: Default::default() };

                system.element_mut::<StringElement>(simulation.string_element_upper).set_compression_factor(model.settings.string_compression_factor);
                system.element_mut::<StringElement>(simulation.string_element_lower).set_compression_factor(model.settings.string_compression_factor);
                system.reset_forces();

                let nock_dof = simulation.nock_node.y();
                let stop_condition = StopCondition::Acceleration(nock_dof, -model.settings.arrow_clamp_force/simulation.arrow_mass, -1);

                let mut brace_crossing_time = f64::INFINITY;
                let mut estimated = true;
                let mut progress = 0.0;

                let mut solver = DynamicSolver::new(&mut system, tolerances, settings);
                let mut states = StateVec::new();

                solver.solve(stop_condition, &mut |system, eval| {
                    let state = simulation.get_bow_state(system, eval, 0.0);

                    if estimated {
                        let ut = state.arrow_pos;
                        let u0 = simulation.geometry.draw.draw_pos;
                        let uT = simulation.geometry.draw.brace_pos;

                        if ut < uT {
                            brace_crossing_time = FRAC_PI_2*state.time/f64::acos((ut - uT)/(u0 - uT));
                        } else {
                            brace_crossing_time = state.time;
                            estimated = false;
                        }
                    }

                    progress = f64::max(progress, state.time/(model.settings.timespan_factor*brace_crossing_time));
                    states.push(state);
                    callback(SimulationMode::Dynamic, 100.0*progress)
                }).map_err(ModelError::SimulationDynamicSolutionFailed)?;

                let state = states.iter().next_back().unwrap();
                simulation.arrow_departure = Some((states.len() - 1, *state.time, *state.arrow_pos, *state.arrow_vel));

                system.element_mut::<MassElement>(simulation.mass_element_arrow).set_mass(0.0);

                if model.settings.timespan_factor > 1.0 {
                    let start_time = system.get_time();
                    let end_time = model.settings.timespan_factor*brace_crossing_time;
                    let stop_condition = StopCondition::Time(end_time);

                    let mut solver = DynamicSolver::new(&mut system, tolerances, settings);
                    solver.solve(stop_condition, &mut |system, eval| {
                        if system.get_time() > start_time {
                            let state = simulation.get_bow_state(system, eval, 0.0);
                            progress = state.time/end_time;
                            states.push(state);
                        }
                        callback(SimulationMode::Dynamic, 100.0*progress)
                    }).map_err(ModelError::SimulationDynamicSolutionFailed)?;
                }

                let damping_energy_limbs = cumulative_simpson(&states.time, &states.damping_power_limbs);
                let damping_energy_string = cumulative_simpson(&states.time, &states.damping_power_string);
                states.damping_energy_limbs = damping_energy_limbs;
                states.damping_energy_string = damping_energy_string;

                let arrow_departure = simulation.arrow_departure.map(|(index, _, _, _)| {
                    ArrowDeparture {
                        state_idx: index,
                        arrow_pos: states.arrow_pos[index],
                        arrow_vel: states.arrow_vel[index],
                        kinetic_energy_arrow: states.kinetic_energy_arrow[index],
                        elastic_energy_limbs: states.elastic_energy_limbs[index],
                        kinetic_energy_limbs: states.kinetic_energy_limbs[index],
                        damping_energy_limbs: states.damping_energy_limbs[index],
                        elastic_energy_string: states.elastic_energy_string[index],
                        kinetic_energy_string: states.kinetic_energy_string[index],
                        damping_energy_string: states.damping_energy_string[index],
                        energy_efficiency: states.kinetic_energy_arrow[index]/statics.final_drawing_work,
                    }
                });

                let max_forces = MaxForces::from_states(&states);
                let max_stresses = MaxStresses::from_states(&states);

                Some(Dynamics {
                    states,
                    arrow_mass: simulation.arrow_mass,
                    arrow_departure,
                    max_forces,
                    max_stresses,
                })
            } else {
                None
            }
        };

        Ok(BowResult { common, statics: Some(statics), dynamics })
    }

    pub fn simulate_statics(model: &'a BowModel) -> Result<BowResult, ModelError> {
        Self::simulate(model, SimulationMode::Static, |_, _| true)
    }

    pub fn simulate_dynamics(model: &'a BowModel) -> Result<BowResult, ModelError> {
        Self::simulate(model, SimulationMode::Dynamic, |_, _| true)
    }

    pub fn simulate_limb_modes(model: &'a BowModel) -> Result<(Common, Vec<Mode>), ModelError> {
        let (mut system, _simulation, common) = Self::initialize(model, false, true)?;
        let modes = natural_frequencies(&mut system).map_err(ModelError::SimulationEigenSolutionFailed)?;
        Ok((common, modes))
    }

    /// Apply a static cantilever load (Fx, Fy, Mz) to the upper limb tip. Used
    /// only by the test suite for cross-validation against analytic results.
    pub fn simulate_static_limb(model: &'a BowModel, Fx: f64, Fy: f64, Mz: f64) -> Result<(Common, State), ModelError> {
        let (mut system, simulation, common) = Self::initialize(model, false, false)?;

        if let Some(node) = simulation.upper.nodes.last() {
            system.add_force(node.x(), move |_t| Fx);
            system.add_force(node.y(), move |_t| Fy);
            system.add_force(node.φ(), move |_t| Mz);
        }

        let s_eval_total = *simulation.geometry.upper.s_eval.last().unwrap();
        let tolerances = StaticTolerances::new(s_eval_total, FRAC_PI_2, model.settings.static_iteration_tolerance);
        let settings = NewtonSettings::default();

        let solver = LoadControl::new(&mut system, tolerances, settings);
        let mut states = StateVec::new();

        solver.solve_equilibrium_path(model.settings.min_draw_resolution, &mut |system, eval, _info| {
            let state = simulation.get_bow_state(system, eval, 0.0);
            states.push(state);
            true
        }).map_err(ModelError::SimulationStaticSolutionFailed)?;

        Ok((common, states.pop().unwrap()))
    }

    /// Collect FEM evaluation points (positions, velocities, strains, forces)
    /// for one limb chain.  For the lower limb the data is returned in the
    /// limb's reflected world frame (x already negated, φ already adjusted).
    /// When the chain has grip beam elements (Handle::Beam), they are
    /// included FIRST so the result viewer renders one continuous outline
    /// from the pivot through the grip and into the limb.
    fn eval_chain(&self, system: &System, chain: &LimbChain) -> (Vec<SVector<f64, 3>>, Vec<SVector<f64, 3>>, Vec<SVector<f64, 3>>, Vec<SVector<f64, 3>>) {
        let mut pos = Vec::new();
        let mut vel = Vec::new();
        let mut strain = Vec::new();
        let mut force = Vec::new();
        for &element in chain.grip_elements.iter().chain(chain.elements.iter()) {
            let element = system.element_ref::<BeamElement>(element);
            element.eval_properties().for_each(|e| {
                pos.push(e.position);
                vel.push(e.velocity);
                strain.push(e.strains);
                force.push(e.forces);
            });
        }
        (pos, vel, strain, force)
    }

    fn layer_results(&self, side: LimbSide, limb_strain: &[SVector<f64, 3>]) -> (Vec<Vec<[f64; 2]>>, Vec<Vec<[f64; 2]>>) {
        let half = match side { LimbSide::Upper => &self.geometry.upper, LimbSide::Lower => &self.geometry.lower };
        let chain = match side { LimbSide::Upper => &self.upper, LimbSide::Lower => &self.lower };
        let n_layers = self.input.section.for_side(side).layers.len();
        let mut layer_strain: Vec<Vec<[f64; 2]>> = vec![Vec::with_capacity(limb_strain.len()); n_layers];
        let mut layer_stress: Vec<Vec<[f64; 2]>> = vec![Vec::with_capacity(limb_strain.len()); n_layers];

        // The strain array is laid out as `[grip eval points] [limb eval
        // points]`. The grip uses a different cross-section (and may have a
        // different layer count); we don't currently project grip strains
        // onto the limb's layer basis. Emit zeros for the grip portion so
        // downstream stress heatmaps remain length-consistent with the
        // position arrays. Limb-layer strain visualisation is unchanged.
        let n_grip = chain.grip_eval.as_ref().map(|g| g.length.len()).unwrap_or(0);
        for _ in 0..n_grip {
            for j in 0..n_layers {
                layer_strain[j].push([0.0, 0.0]);
                layer_stress[j].push([0.0, 0.0]);
            }
        }

        for i in 0..half.strain_eval.len() {
            let strain_i = &limb_strain[n_grip + i];
            half.strain_eval[i].iter().tuples().enumerate().for_each(|(j, (eval_back, eval_belly))| {
                layer_strain[j].push([eval_back.dot(strain_i), eval_belly.dot(strain_i)]);
            });
            half.stress_eval[i].iter().tuples().enumerate().for_each(|(j, (eval_back, eval_belly))| {
                layer_stress[j].push([eval_back.dot(strain_i), eval_belly.dot(strain_i)]);
            });
        }

        (layer_strain, layer_stress)
    }

    fn get_bow_state(&self, system: &System, eval: &SystemEval, draw_stiffness: f64) -> State {
        let nock_dof_y = self.nock_node.y();
        let time = system.get_time();
        let draw_length = self.geometry.draw.draw_ref - system.get_position(nock_dof_y);
        let power_stroke = self.geometry.draw.brace_pos - system.get_position(nock_dof_y);
        // Single-point external force at the nock — no symmetry doubling.
        let draw_force = -eval.get_external_force(nock_dof_y);

        let (arrow_acc, arrow_vel, arrow_pos) = if let Some((_, t0, s0, v0)) = self.arrow_departure {
            (0.0, v0, s0 + v0*(time - t0))
        } else {
            (
                eval.get_acceleration(nock_dof_y),
                system.get_velocity(nock_dof_y),
                system.get_position(nock_dof_y),
            )
        };

        // Concatenate contact positions/velocities of both string halves.
        // The first entry is always the NOCK (so consumers can read
        // `string_pos[0]` as the arrow position, matching v4 layout); the
        // upper-half contacts follow (going outward to upper_tip); then the
        // lower-half contacts (going outward to lower_tip).
        let mut string_pos = Vec::<SVector<f64, 2>>::new();
        let mut string_vel = Vec::<SVector<f64, 2>>::new();
        {
            let elem_u = system.element_ref::<StringElement>(self.string_element_upper);
            let pos_u: Vec<_> = elem_u.contact_positions().collect();
            let vel_u: Vec<_> = elem_u.contact_velocities().collect();
            // Upper element runs [nock, …, upper_tip]; first contact is the nock.
            string_pos.extend(pos_u);
            string_vel.extend(vel_u);
        }
        {
            let elem_l = system.element_ref::<StringElement>(self.string_element_lower);
            // Lower element runs [lower_tip, …, nock]; reverse and drop the
            // leading nock to append the lower-side string contacts going out.
            let pos_l: Vec<_> = elem_l.contact_positions().collect();
            let vel_l: Vec<_> = elem_l.contact_velocities().collect();
            string_pos.extend(pos_l.into_iter().rev().skip(1));
            string_vel.extend(vel_l.into_iter().rev().skip(1));
        }

        let (upper_pos, upper_vel, upper_strain, upper_force) = self.eval_chain(system, &self.upper);
        let (lower_pos, lower_vel, lower_strain, lower_force) = self.eval_chain(system, &self.lower);

        let (upper_layer_strain, upper_layer_stress) = self.layer_results(LimbSide::Upper, &upper_strain);
        let (lower_layer_strain, lower_layer_stress) = self.layer_results(LimbSide::Lower, &lower_strain);

        // Grip force: y-component reaction at both limb roots, with the
        // pressure-positive sign convention (limbs push down on the grip).
        let grip_force_upper = -(upper_force[0][0]*f64::sin(upper_pos[0][2]) + upper_force[0][1]*f64::cos(upper_pos[0][2]));
        let grip_force_lower = -(lower_force[0][0]*f64::sin(lower_pos[0][2]) + lower_force[0][1]*f64::cos(lower_pos[0][2]));
        let grip_force = grip_force_upper + grip_force_lower;

        let elastic_energy_limbs = self.upper.elements.iter().chain(self.lower.elements.iter())
            .map(|&e| system.element_ref::<BeamElement>(e).potential_energy())
            .sum::<f64>();
        let elastic_energy_string =
              system.element_ref::<StringElement>(self.string_element_upper).potential_energy()
            + system.element_ref::<StringElement>(self.string_element_lower).potential_energy();

        let kinetic_energy_limbs = self.upper.elements.iter().chain(self.lower.elements.iter())
            .map(|&e| system.element_ref::<BeamElement>(e).kinetic_energy())
            .sum::<f64>()
            + system.element_ref::<MassElement>(self.upper.mass_element_tip).kinetic_energy()
            + system.element_ref::<MassElement>(self.lower.mass_element_tip).kinetic_energy();
        let kinetic_energy_string = system.element_ref::<MassElement>(self.mass_element_nock).kinetic_energy()
            + system.element_ref::<MassElement>(self.upper.mass_element_string_tip).kinetic_energy()
            + system.element_ref::<MassElement>(self.lower.mass_element_string_tip).kinetic_energy();
        let kinetic_energy_arrow = 0.5*self.arrow_mass*arrow_vel.powi(2);

        let damping_power_limbs = self.upper.elements.iter().chain(self.lower.elements.iter())
            .map(|&e| system.element_ref::<BeamElement>(e).dissipative_power())
            .sum::<f64>();
        let damping_power_string =
              system.element_ref::<StringElement>(self.string_element_upper).dissipative_power()
            + system.element_ref::<StringElement>(self.string_element_lower).dissipative_power();

        let string_length =
              system.element_ref::<StringElement>(self.string_element_upper).get_current_length()
            + system.element_ref::<StringElement>(self.string_element_lower).get_current_length();
        // Both halves carry the same tension in equilibrium; pick the upper.
        let string_force = system.element_ref::<StringElement>(self.string_element_upper).normal_force_total();
        let strand_force = string_force/(self.input.string.n_strands as f64);

        // String tip angles on each limb (angle between limb tip tangent and
        // string at the tip).
        let node_pos2 = |n: Node| -> SVector<f64, 2> {
            vector![system.get_position(n.x()), system.get_position(n.y())]
        };
        let upper_tip_node = *self.upper.nodes.last().unwrap();
        let upper_tip_inner = self.upper.nodes[self.upper.nodes.len() - 2];
        let lower_tip_node = *self.lower.nodes.last().unwrap();
        let lower_tip_inner = self.lower.nodes[self.lower.nodes.len() - 2];
        let nock_pos = node_pos2(self.nock_node);
        let _ = nock_pos;

        let string_tip_angle_upper = {
            let dir_limb: SVector<f64, 2> = (upper_pos[upper_pos.len() - 1] - upper_pos[upper_pos.len() - 2]).fixed_rows::<2>(0).into();
            let dir_string: SVector<f64, 2> = node_pos2(upper_tip_inner) - node_pos2(upper_tip_node);
            dir_limb.angle(&dir_string)
        };
        let string_tip_angle_lower = {
            let dir_limb: SVector<f64, 2> = (lower_pos[lower_pos.len() - 1] - lower_pos[lower_pos.len() - 2]).fixed_rows::<2>(0).into();
            let dir_string: SVector<f64, 2> = node_pos2(lower_tip_inner) - node_pos2(lower_tip_node);
            dir_limb.angle(&dir_string)
        };

        // String break-angle at the nock: angle between the upper-side and
        // lower-side string segments measured at the nock.  We use the FIRST
        // string contact AFTER the nock on each side: for the upper element
        // contact[1] (nock is contact[0]); for the lower element which runs
        // [tip → … → nock], the contact second-to-last is the neighbour of the
        // nock.
        let (dir_upper_at_nock, dir_lower_at_nock) = {
            let elem_u = system.element_ref::<StringElement>(self.string_element_upper);
            let pu: Vec<_> = elem_u.contact_positions().collect();
            let elem_l = system.element_ref::<StringElement>(self.string_element_lower);
            let pl: Vec<_> = elem_l.contact_positions().collect();
            // Upper: contact[0] = nock, contact[1] = nearest neighbour.
            let du = pu[1] - pu[0];
            // Lower: contact[last] = nock, contact[last-1] = nearest neighbour.
            let dl = pl[pl.len() - 2] - pl[pl.len() - 1];
            (du, dl)
        };
        let string_center_angle = f64::atan2(dir_upper_at_nock[0], dir_upper_at_nock[1])
                                - f64::atan2(dir_lower_at_nock[0], dir_lower_at_nock[1]);

        State {
            time,
            draw_length,
            power_stroke,

            limb_pos: upper_pos,
            limb_vel: upper_vel,
            lower_limb_pos: lower_pos,
            lower_limb_vel: lower_vel,

            string_pos,
            string_vel,

            limb_strain: upper_strain,
            limb_force: upper_force,
            lower_limb_strain: lower_strain,
            lower_limb_force: lower_force,

            layer_strain: upper_layer_strain,
            layer_stress: upper_layer_stress,
            lower_layer_strain,
            lower_layer_stress,

            arrow_pos,
            arrow_vel,
            arrow_acc,

            elastic_energy_limbs,
            elastic_energy_string,

            kinetic_energy_limbs,
            kinetic_energy_string,
            kinetic_energy_arrow,

            damping_energy_limbs: 0.0,
            damping_energy_string: 0.0,
            damping_power_limbs,
            damping_power_string,

            draw_force,
            draw_stiffness,
            grip_force,
            string_length,
            string_tip_angle: string_tip_angle_upper,
            string_tip_angle_lower,
            string_center_angle,
            string_force,
            strand_force,
        }
    }
}

// Input dimensions: (state, layer, length, belly/back) -> (value, [layer, length, belly/back])
pub fn find_max_layer_result(input: &[Vec<Vec<[f64; 2]>>], i_layer: usize) -> (f64, [usize; 3]) {
    let n_states = input.len();
    let n_length = input[0][0].len();
    discrete_maximum_nd(&mut |i| input[i[0]][i_layer][i[1]][i[2]], [n_states, n_length, 2])
}

pub fn find_min_layer_result(input: &[Vec<Vec<[f64; 2]>>], i_layer: usize) -> (f64, [usize; 3]) {
    let n_states = input.len();
    let n_length = input[0][0].len();
    discrete_minimum_nd(&mut |i| input[i[0]][i_layer][i[1]][i[2]], [n_states, n_length, 2])
}

/// Prepend a `Handle::Beam` grip's static eval data to a limb's `LimbInfo`.
/// When `grip` is `None` (Rigid / Flexible handle) the limb info is returned
/// unchanged. The grip's `length` values are kept as-is (they live on the
/// arc-length range `[0, L_grip]`); the limb's `length` values are shifted
/// by `L_grip` so the combined parametrisation is monotonic. Other arrays
/// are simply concatenated.
fn prepend_grip_to_limb_info(grip: &Option<GripEvalInfo>, mut limb: crate::output::LimbInfo) -> crate::output::LimbInfo {
    let g = match grip {
        Some(g) => g,
        None => return limb,
    };
    let l_grip = g.length.last().copied().unwrap_or(0.0);

    let mut length = g.length.clone();
    length.extend(limb.length.iter().map(|s| s + l_grip));
    let mut width = g.width.clone();             width.extend(limb.width.iter().copied());
    let mut height = g.height.clone();           height.extend(limb.height.iter().copied());
    let mut bounds = g.bounds.clone();           bounds.extend(limb.bounds.iter().cloned());
    let mut ratio = g.ratio.clone();             ratio.extend(limb.ratio.iter().copied());
    let mut heights = g.heights.clone();         heights.extend(limb.heights.iter().cloned());
    let mut position_eval = g.position_eval.clone();       position_eval.extend(limb.position_eval.iter().copied());
    let mut position_control = g.position_control.clone(); position_control.extend(limb.position_control.iter().copied());
    let mut curvature_eval = g.curvature_eval.clone();     curvature_eval.extend(limb.curvature_eval.iter().copied());

    limb.length = length;
    limb.width = width;
    limb.height = height;
    limb.bounds = bounds;
    limb.ratio = ratio;
    limb.heights = heights;
    limb.position_eval = position_eval;
    limb.position_control = position_control;
    limb.curvature_eval = curvature_eval;
    limb
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{
        ArrowMass, BowModel, BowString, Damping, Draw, DrawLength, Handle, Height, Layer,
        LayerAlignment, LimbSection, Line, Masses, Material, Profile, ProfileSegment, RigidHandle,
        Section, Settings, Width,
    };

    /// Build a yumi-style asymmetric bow programmatically and simulate it.
    /// The yumi has an upper limb roughly twice as long as the lower limb,
    /// with the grip located ~⅓ from the bottom (positive nock offset shift).
    fn build_yumi() -> BowModel {
        let upper_limb_len = 1.5_f64;
        let lower_limb_len = 0.75_f64;

        let upper_section = LimbSection {
            alignment: LayerAlignment::SectionCenter,
            width: Width::linear(0.025, 0.012),
            layers: vec![Layer::new("Bamboo", "BambooMaterial", Height::linear(0.022, 0.014))],
        };
        let lower_section = LimbSection {
            alignment: LayerAlignment::SectionCenter,
            width: Width::linear(0.025, 0.012),
            layers: vec![Layer::new("Bamboo", "BambooMaterial", Height::linear(0.022, 0.014))],
        };

        BowModel {
            comment: "Yumi-style asymmetric bow".into(),
            settings: Settings {
                num_limb_elements: 30,
                num_limb_eval_points: 100,
                ..Settings::default()
            },
            handle: Handle::Rigid(RigidHandle {
                length_upper: 0.10,
                length_lower: 0.05,
                angle: 0.0,
                pivot: 0.0,
            }),
            draw: Draw {
                brace_height: 0.18,
                draw_length: DrawLength::Standard(0.85),
                // Nock above the geometric center to model the natural asymmetry
                nock_offset: 0.10,
            },
            section: Section {
                materials: vec![Material {
                    name: "BambooMaterial".into(),
                    color: "#a07851".into(),
                    density: 700.0,
                    youngs_modulus: 18e9,
                    shear_modulus: 6e9,
                    tensile_strength: 100e6,
                    compressive_strength: 60e6,
                    safety_margin: 1.0,
                }],
                upper: upper_section,
                lower: lower_section,
            },
            profile: Profile {
                upper: vec![ProfileSegment::Line(Line::new(upper_limb_len))],
                lower: vec![ProfileSegment::Line(Line::new(lower_limb_len))],
            },
            string: BowString {
                n_strands: 12,
                strand_density: 0.0005,
                strand_stiffness: 3500.0,
            },
            masses: Masses {
                arrow: ArrowMass::Mass(0.025),
                limb_tip_upper: 0.005,
                limb_tip_lower: 0.005,
                string_nock: 0.005,
                string_tip_upper: 0.003,
                string_tip_lower: 0.003,
            },
            damping: Damping {
                damping_ratio_limbs: 0.05,
                damping_ratio_string: 0.05,
            },
        }
    }

    #[test]
    fn test_yumi_static_simulation() {
        let model = build_yumi();
        let result = Simulation::simulate_statics(&model)
            .expect("yumi static simulation should succeed");
        let statics = result.statics.expect("statics result");

        // Sanity: final draw force is positive and reasonable.
        assert!(statics.final_draw_force > 1.0, "draw_force={}", statics.final_draw_force);
        assert!(statics.final_draw_force < 1000.0, "draw_force={}", statics.final_draw_force);
        // Drawing work is positive.
        assert!(statics.final_drawing_work > 0.0);
        // Storage factor positive (yumi straight-limbs can give >1 if numerical FD is concave).
        assert!(statics.storage_factor > 0.0 && statics.storage_factor < 2.0,
            "storage={}", statics.storage_factor);

        // The asymmetric bow may have a non-zero residual draw_force at brace
        // (the current bracing algorithm balances upper-string slope, not the
        // net nock force).  We only require the FD curve to be monotonic and
        // the final draw force to dominate the bracing residual.
        let states = &statics.states;
        let first = *states.draw_force.first().unwrap();
        let last = *states.draw_force.last().unwrap();
        assert!(last > first.abs() * 2.0,
            "expected final draw_force >> bracing residual, got first={}, last={}", first, last);
    }

    #[test]
    fn test_yumi_dynamic_simulation() {
        let model = build_yumi();
        let result = Simulation::simulate_dynamics(&model)
            .expect("yumi dynamic simulation should succeed");
        let dyn_ = result.dynamics.expect("dynamics result");
        let dep = dyn_.arrow_departure.expect("arrow departed");
        // Some kinetic energy was transferred to the arrow.
        assert!(dep.kinetic_energy_arrow > 0.0);
        // Energy efficiency in (0, 1).
        assert!(dep.energy_efficiency > 0.0 && dep.energy_efficiency < 1.0,
            "efficiency={}", dep.energy_efficiency);
        // Arrow velocity is reasonable (yumi arrow speed ~ 50 m/s).
        assert!(dep.arrow_vel.abs() > 5.0 && dep.arrow_vel.abs() < 200.0,
            "arrow_vel={}", dep.arrow_vel);
    }
}
