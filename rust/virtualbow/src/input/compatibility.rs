use std::fs::File;
use std::path::Path;

use serde::{Serialize, Deserialize};

use crate::errors::ModelError;

use super::versions::latest;
use super::versions::version4;
use super::versions::version3;
use super::versions::version2;
use super::versions::version1;
use super::versions::legacy_symmetric;

// TODO: Use env!("CARGO_PKG_VERSION") for the latest bow model variant instead of hard-coding the
// version as soon as https://github.com/serde-rs/serde/issues/2485 gets solved.
// Inspired by https://stackoverflow.com/a/70380491
#[derive(Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum BowModelVersion {
    #[serde(rename = "0.11.0")]
    Latest(latest::BowModel),

    #[serde(rename = "0.10.0")]
    Version4(version4::BowModel),

    // Original upstream VirtualBow 0.10 single-limb ("symmetric") schema.
    // Shares the "0.10.0" version tag with `Version4` but has an incompatible
    // layout, so it cannot be matched by tag. It is instead recognised via a
    // manual fallback in `load` and never appears in a serialized file, hence
    // `skip`.
    #[serde(skip)]
    LegacySymmetric(legacy_symmetric::BowModel),

    #[serde(rename = "0.9.1", alias = "0.9")]
    Version3(version3::BowModel),

    #[serde(rename = "0.8")]
    Version2(version2::BowModel),

    #[serde(rename = "0.7.1", alias = "0.7")]
    Version1(version1::BowModel),

    #[serde(alias = "0.6.1", alias = "0.6", alias = "0.5", alias = "0.4", alias = "0.3", alias = "0.2", alias="0.1")]
    Unsupported,

    #[serde(other)]
    Unrecognized,
}

impl BowModelVersion {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ModelError> {
        let file = File::open(&path).map_err(|e| ModelError::InputLoadFileError(path.as_ref().to_owned(), e))?;
        let value: serde_json::Value = serde_json::from_reader(file).map_err(ModelError::InputDeserializeJsonError)?;

        match serde_json::from_value::<Self>(value.clone()) {
            Ok(version) => Ok(version),
            Err(err) => {
                // The tagged parse can fail for a "0.10.0" file that actually
                // uses the original upstream single-limb schema (which carries
                // a `dimensions` block instead of `handle`/`draw`). Retry it as
                // the legacy symmetric schema before surfacing the error.
                let is_legacy_symmetric = value.get("version").and_then(|v| v.as_str()) == Some("0.10.0")
                    && value.get("dimensions").is_some();

                if is_legacy_symmetric {
                    let model = serde_json::from_value::<legacy_symmetric::BowModel>(value)
                        .map_err(ModelError::InputDeserializeJsonError)?;
                    Ok(Self::LegacySymmetric(model))
                } else {
                    Err(ModelError::InputDeserializeJsonError(err))
                }
            }
        }
    }

    pub fn save<P: AsRef<Path>>(path: P, model: &latest::BowModel) -> Result<(), ModelError> {
        let mut file = File::create(&path).map_err(|e| ModelError::InputSaveFileError(path.as_ref().to_owned(), e))?;
        let version = BowModelVersion::Latest(model.clone());    // TODO: Unnecessary clone?
        serde_json::to_writer_pretty(&mut file, &version).map_err(ModelError::InputSerializeJsonError)?;
        
        Ok(())
    }

    pub fn is_latest(&self) -> bool {
        matches!(self, Self::Latest(_))
    }

    pub fn get_latest(self) -> Result<latest::BowModel, ModelError> {
        match self {
            Self::Latest(model) => Ok(model),
            Self::LegacySymmetric(model) => Ok(latest::BowModel::from(model)),
            Self::Version4(model) => Ok(latest::BowModel::from(model)),
            Self::Version3(model) => Ok(latest::BowModel::from(version4::BowModel::from(model))),
            Self::Version2(model) => Ok(latest::BowModel::from(version4::BowModel::from(version3::BowModel::from(model)))),
            Self::Version1(model) => Ok(latest::BowModel::from(version4::BowModel::from(version3::BowModel::from(version2::BowModel::from(model))))),
            Self::Unsupported => Err(ModelError::InputVersionUnsupported),
            Self::Unrecognized => Err(ModelError::InputVersionUnrecognized),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::input::BowModel;

    // The original upstream single-limb ("symmetric") 0.10 schema must still
    // load, even though it shares the "0.10.0" version tag with the fork's
    // (incompatible) `version4` schema. recurve.bow is such a file.
    #[test]
    fn loads_legacy_symmetric_recurve() {
        let model = BowModel::load("../../docs/examples/bows/recurve.bow")
            .expect("legacy symmetric recurve.bow should load");

        // The single limb is mirrored into both halves and flagged symmetric.
        assert!(model.symmetry.profile && model.symmetry.width && model.symmetry.layers);
        assert!(model.profile.upper == model.profile.lower);
        assert!(model.section.upper == model.section.lower);

        // Settings missing from the upstream schema get the default tolerance.
        assert!(model.settings.static_iteration_tolerance == 1e-6);
        assert!(model.settings.dynamic_iteration_tolerance == 1e-6);

        // The converted model passes full validation.
        model.validate().expect("converted recurve model should be valid");
    }
}