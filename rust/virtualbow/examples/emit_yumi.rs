//! Emits a canonical yumi-style asymmetric BowModel to disk.
//!
//! The same parameters used by `simulation.rs::tests::build_yumi()` are
//! materialised here so we can ship a real on-disk example. The binary writes
//! the model to BOTH:
//!   - `rust/virtualbow/data/examples/yumi.bow`
//!   - `docs/examples/bows/yumi.bow`
//!
//! Run with `cargo run --release --example emit_yumi -p virtualbow`.

use std::path::PathBuf;
use virtualbow::input::{
    ArrowMass, BowModel, BowString, Damping, Draw, DrawLength, Handle, Height, Layer,
    LayerAlignment, LimbSection, Line, Masses, Material, Profile, ProfileSegment, RigidHandle,
    Section, Settings, Symmetry, Width,
};

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
        comment: "Yumi-style asymmetric bow (canonical example, v0.11.0).".into(),
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
        symmetry: Symmetry::default(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = build_yumi();
    model.validate()?;

    // Targets are relative to the workspace root, derived from CARGO_MANIFEST_DIR
    // (which points at rust/virtualbow/).
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(2)
        .ok_or("could not locate workspace root")?
        .to_path_buf();

    let targets = [
        manifest_dir.join("data").join("examples").join("yumi.bow"),
        workspace_root.join("docs").join("examples").join("bows").join("yumi.bow"),
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
