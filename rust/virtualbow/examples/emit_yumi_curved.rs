//! Emits a curved yumi-style asymmetric BowModel that matches the silhouette
//! shown in the reference photograph (a real bamboo-laminated yumi).
//!
//! Differences from the canonical `yumi.bow`:
//!  * Both limbs have a clearly reflexed profile in the unbraced state
//!    (modeled with cubic-spline segments).
//!  * Each limb tip has a small back-recurve flick.
//!  * The cross-section is a three-layer laminate: bamboo back, wood core,
//!    bamboo belly — the traditional construction of a Japanese yumi.
//!
//! The output is written to:
//!   - `rust/virtualbow/data/examples/yumi_curved.bow`
//!   - `docs/examples/bows/yumi_curved.bow`
//!
//! Run with `cargo run --release --example emit_yumi_curved -p virtualbow`.

use std::path::PathBuf;
use virtualbow::input::{
    Arc, ArrowMass, BowModel, BowString, Damping, Draw, DrawLength, Handle, Height, Layer,
    LayerAlignment, LimbSection, Masses, Material, Profile, ProfileSegment, RigidHandle, Section,
    Settings, Spline, Width,
};

/// Lengths along the limb (m).
const UPPER_LIMB_LEN: f64 = 1.55;
const LOWER_LIMB_LEN: f64 = 0.78;

/// Length reserved at each limb tip for the back-recurve flick (m).
const TIP_RECURVE_LEN: f64 = 0.10;
/// Radius of the back-recurve flick at each tip (m). Positive radius bends
/// the curve toward the bow's BACK (away from the archer), which is the
/// classic recurve flick that catches the string when the bow is braced.
const TIP_RECURVE_RADIUS: f64 = 0.18;

/// Reflex bulge amplitude (m) of the limb body, measured perpendicular to the
/// straight grip-to-tip direction. Positive y in the limb's local frame is
/// the bow's back (the side the archer faces away from), so a positive bulge
/// means the unbraced limbs already curve away from the archer — the
/// definition of a reflexed/recurved bow. The yumi's classic shape has the
/// upper limb reflexing more than the lower limb because it is longer and
/// does most of the work.
const UPPER_REFLEX: f64 = 0.085;
const LOWER_REFLEX: f64 = 0.050;

/// Construct the spline-based body of one limb plus a small recurved tip.
///
/// The body is a cubic spline through five (x, y) control points expressed in
/// the limb's local frame (x along the limb, y perpendicular toward the back
/// of the bow). The limb starts straight at the grip, bows out to its peak
/// reflex roughly halfway, then comes back down to the tip-recurve junction
/// running approximately tangent to +x. The tip is then a short circular arc
/// with negative (back-bending) radius.
fn build_limb_segments(total_len: f64, reflex: f64) -> Vec<ProfileSegment> {
    let body_len = total_len - TIP_RECURVE_LEN;

    // Five control points, evenly spaced along the limb body. The y values
    // describe a smooth bell-shaped reflex with its peak slightly past the
    // mid-point (more like the photographed yumi).
    let body = Spline {
        points: vec![
            [0.00 * body_len, 0.0],
            [0.30 * body_len, 0.55 * reflex],
            [0.55 * body_len, reflex],
            [0.80 * body_len, 0.50 * reflex],
            [body_len, 0.0],
        ],
    };

    let tip = Arc {
        length: TIP_RECURVE_LEN,
        radius: TIP_RECURVE_RADIUS,
    };

    vec![ProfileSegment::Spline(body), ProfileSegment::Arc(tip)]
}

fn build_yumi_curved() -> BowModel {
    // Three-layer laminate: bamboo / wood / bamboo. The cross-section is
    // section-centered; widths taper smoothly toward the tips.
    let layers = vec![
        Layer::new(
            "Bamboo (back)",
            "Bamboo",
            Height::new(vec![[0.0, 0.0040], [1.0, 0.0030]]),
        ),
        Layer::new(
            "Wood core",
            "Wood",
            Height::new(vec![[0.0, 0.0140], [1.0, 0.0090]]),
        ),
        Layer::new(
            "Bamboo (belly)",
            "Bamboo",
            Height::new(vec![[0.0, 0.0040], [1.0, 0.0030]]),
        ),
    ];

    let upper_section = LimbSection {
        alignment: LayerAlignment::SectionCenter,
        width: Width::new(vec![[0.0, 0.024], [0.5, 0.020], [1.0, 0.011]]),
        layers: layers.clone(),
    };
    let lower_section = LimbSection {
        alignment: LayerAlignment::SectionCenter,
        width: Width::new(vec![[0.0, 0.024], [0.5, 0.020], [1.0, 0.011]]),
        layers: layers.clone(),
    };

    BowModel {
        comment: "Curved yumi with reflex profile and a three-layer bamboo/wood/bamboo laminate."
            .into(),
        settings: Settings {
            num_limb_elements: 80,
            num_limb_eval_points: 250,
            max_timestep: 5e-5,
            steps_per_period: 500,
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
            nock_offset: 0.10,
        },
        section: Section {
            materials: vec![
                Material {
                    name: "Bamboo".into(),
                    color: "#d9b377".into(),
                    density: 700.0,
                    youngs_modulus: 18.0e9,
                    shear_modulus: 6.0e9,
                    tensile_strength: 130e6,
                    compressive_strength: 70e6,
                    safety_margin: 1.0,
                },
                Material {
                    name: "Wood".into(),
                    color: "#8a5a30".into(),
                    density: 600.0,
                    youngs_modulus: 10.0e9,
                    shear_modulus: 3.5e9,
                    tensile_strength: 80e6,
                    compressive_strength: 50e6,
                    safety_margin: 1.0,
                },
            ],
            upper: upper_section,
            lower: lower_section,
        },
        profile: Profile {
            upper: build_limb_segments(UPPER_LIMB_LEN, UPPER_REFLEX),
            lower: build_limb_segments(LOWER_LIMB_LEN, LOWER_REFLEX),
        },
        string: BowString {
            n_strands: 14,
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = build_yumi_curved();
    model.validate()?;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(2)
        .ok_or("could not locate workspace root")?
        .to_path_buf();

    let targets = [
        manifest_dir
            .join("data")
            .join("examples")
            .join("yumi_curved.bow"),
        workspace_root
            .join("docs")
            .join("examples")
            .join("bows")
            .join("yumi_curved.bow"),
    ];

    for target in &targets {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        model.save(target)?;
        println!("wrote {}", target.display());
    }

    Ok(())
}
