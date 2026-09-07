use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ErrorCode, FormatWrightError, Result, Stage};

pub const PRESET_SCHEMA_VERSION: u16 = 2;

/// One Explorer convert-verb assignment (spec E-06). Verb IDs are the fixed
/// baseline set from `explorer-verbs.json`; a binding only toggles the verb
/// and optionally points it at a library preset so right-click conversions
/// use the user's parameters. Bindings live inside the preset library so
/// they travel with preset export/import.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShellVerbBinding {
    pub verb_id: String,
    #[serde(default = "default_shell_verb_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub preset_id: Option<Uuid>,
}

const fn default_shell_verb_enabled() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversionPreset {
    pub schema_version: u16,
    pub preset_id: Uuid,
    pub name: String,
    pub target_format: String,
    pub quality: Option<u8>,
    pub width: Option<u32>,
    pub dpi: Option<u16>,
    pub color_mode: Option<String>,
    #[serde(default)]
    pub video_crf: Option<u8>,
    #[serde(default)]
    pub video_preset: Option<String>,
    #[serde(default)]
    pub audio_bitrate_kbps: Option<u32>,
    pub preserve_all_streams: bool,
}

impl ConversionPreset {
    /// Validates the stable preset contract and conservative setting bounds.
    ///
    /// # Errors
    ///
    /// Returns `InputInvalid` when the schema version, name, target, or an
    /// optional conversion setting is outside the public v1 contract.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRESET_SCHEMA_VERSION {
            return Err(invalid_preset(
                "Unsupported preset schema version",
                "Export the preset again from a compatible Anole version.",
            ));
        }
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err(invalid_preset(
                "Preset name must contain 1 to 80 printable characters",
                "Choose a shorter visible name.",
            ));
        }
        if self.target_format.is_empty()
            || self.target_format.len() > 16
            || !self
                .target_format
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err(invalid_preset(
                "Preset target format must be a lowercase format identifier",
                "Use a target such as webp, mp4, pdf, or yaml.",
            ));
        }
        if self
            .quality
            .is_some_and(|quality| !(1..=100).contains(&quality))
        {
            return Err(invalid_preset(
                "Preset quality must be between 1 and 100",
                "Choose a supported quality value.",
            ));
        }
        if self
            .width
            .is_some_and(|width| !(1..=16_384).contains(&width))
        {
            return Err(invalid_preset(
                "Preset width must be between 1 and 16384 pixels",
                "Choose a supported output width.",
            ));
        }
        if self.dpi.is_some_and(|dpi| !(36..=600).contains(&dpi)) {
            return Err(invalid_preset(
                "Preset DPI must be between 36 and 600",
                "Choose a supported rendering resolution.",
            ));
        }
        if self
            .color_mode
            .as_deref()
            .is_some_and(|mode| !matches!(mode, "rgb" | "gray"))
        {
            return Err(invalid_preset(
                "Preset color mode must be rgb or gray",
                "Choose a supported color mode.",
            ));
        }
        if self.video_crf.is_some_and(|crf| crf > 51) {
            return Err(invalid_preset(
                "Preset video CRF must be between 0 and 51",
                "Choose a supported CRF value.",
            ));
        }
        if self.video_preset.as_deref().is_some_and(|preset| {
            !matches!(
                preset,
                "ultrafast"
                    | "superfast"
                    | "veryfast"
                    | "faster"
                    | "fast"
                    | "medium"
                    | "slow"
                    | "slower"
                    | "veryslow"
            )
        }) {
            return Err(invalid_preset(
                "Preset video preset must be an x264 speed preset",
                "Choose a supported encode preset.",
            ));
        }
        if self
            .audio_bitrate_kbps
            .is_some_and(|kbps| !(8..=320).contains(&kbps))
        {
            return Err(invalid_preset(
                "Preset audio bitrate must be between 8 and 320 kbps",
                "Choose a supported bitrate.",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresetLibrary {
    pub schema_version: u16,
    pub presets: Vec<ConversionPreset>,
    /// Explorer verb assignments (E-06). Missing entries mean "enabled with
    /// default parameters", which is also how v1 libraries read.
    #[serde(default)]
    pub shell_verbs: Vec<ShellVerbBinding>,
}

impl PresetLibrary {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            schema_version: PRESET_SCHEMA_VERSION,
            presets: Vec::new(),
            shell_verbs: Vec::new(),
        }
    }

    /// Validates the library envelope, every preset, and unique identities.
    ///
    /// # Errors
    ///
    /// Returns `InputInvalid` for unsupported versions, excessive entry
    /// counts, invalid presets, duplicate IDs, or duplicate names.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRESET_SCHEMA_VERSION {
            return Err(invalid_preset(
                "Unsupported preset-library schema version",
                "Export the library again from a compatible Anole version.",
            ));
        }
        if self.presets.len() > 4096 {
            return Err(invalid_preset(
                "Preset library cannot contain more than 4096 entries",
                "Split the library or remove unused presets.",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        for preset in &self.presets {
            preset.validate()?;
            if !ids.insert(preset.preset_id) {
                return Err(invalid_preset(
                    "Preset library contains a duplicate preset ID",
                    "Remove the duplicate entry and import again.",
                ));
            }
            if !names.insert(preset.name.trim().to_lowercase()) {
                return Err(invalid_preset(
                    "Preset names must be unique",
                    "Rename duplicate presets and import again.",
                ));
            }
        }
        if self.shell_verbs.len() > 64 {
            return Err(invalid_preset(
                "Preset library cannot carry more than 64 shell-verb bindings",
                "Remove unused Explorer menu bindings and import again.",
            ));
        }
        let mut verb_ids = std::collections::BTreeSet::new();
        for binding in &self.shell_verbs {
            if binding.verb_id.is_empty()
                || binding.verb_id.len() > 64
                || binding
                    .verb_id
                    .chars()
                    .any(|c| !c.is_ascii_alphanumeric() && c != '.' && c != '_')
            {
                return Err(invalid_preset(
                    "Shell-verb binding has an invalid verb ID",
                    "Reset the Explorer menu configuration and retry.",
                ));
            }
            if !verb_ids.insert(binding.verb_id.as_str()) {
                return Err(invalid_preset(
                    "Shell-verb binding IDs must be unique",
                    "Reset the Explorer menu configuration and retry.",
                ));
            }
            if let Some(preset_id) = binding.preset_id
                && !self
                    .presets
                    .iter()
                    .any(|preset| preset.preset_id == preset_id)
            {
                return Err(invalid_preset(
                    "Shell-verb binding points at a preset that is not in this library",
                    "Import the preset first, then rebind the Explorer verb.",
                ));
            }
        }
        Ok(())
    }

    /// Upgrades a freshly deserialized v1 library (no verb bindings) to the
    /// current schema in memory; the next save persists v2. Libraries older
    /// than v1 or newer than current stay rejected by `validate`.
    #[must_use]
    pub fn migrate_legacy(mut self) -> Self {
        if self.schema_version == 1 {
            self.schema_version = PRESET_SCHEMA_VERSION;
            self.shell_verbs = Vec::new();
        }
        self
    }

    /// Inserts a new preset or updates the preset with the same stable ID.
    ///
    /// # Errors
    ///
    /// Returns `InputInvalid` when the preset is invalid or its name conflicts
    /// with a different preset.
    pub fn upsert(&mut self, mut preset: ConversionPreset) -> Result<()> {
        let trimmed_name = preset.name.trim().to_owned();
        preset.name = trimmed_name;
        preset.target_format = preset.target_format.to_ascii_lowercase();
        preset.validate()?;
        let mut updated = self.clone();
        if updated.presets.iter().any(|candidate| {
            candidate.preset_id != preset.preset_id
                && candidate.name.to_lowercase() == preset.name.to_lowercase()
        }) {
            return Err(invalid_preset(
                "A preset with this name already exists",
                "Choose a unique preset name or edit the existing preset.",
            ));
        }
        if let Some(existing) = updated
            .presets
            .iter_mut()
            .find(|candidate| candidate.preset_id == preset.preset_id)
        {
            *existing = preset;
        } else {
            updated.presets.push(preset);
        }
        updated.presets.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then(left.preset_id.cmp(&right.preset_id))
        });
        updated.validate()?;
        *self = updated;
        Ok(())
    }

    pub fn remove(&mut self, preset_id: Uuid) -> bool {
        let before = self.presets.len();
        self.presets.retain(|preset| preset.preset_id != preset_id);
        before != self.presets.len()
    }

    /// Atomically merges a validated portable library into this library.
    ///
    /// # Errors
    ///
    /// Returns `InputInvalid` without changing `self` when any imported entry
    /// is invalid or conflicts with an existing preset name.
    pub fn merge(&mut self, imported: Self) -> Result<usize> {
        imported.validate()?;
        let count = imported.presets.len();
        let mut merged = self.clone();
        for preset in imported.presets {
            merged.upsert(preset)?;
        }
        *self = merged;
        Ok(count)
    }
}

impl Default for PresetLibrary {
    fn default() -> Self {
        Self::empty()
    }
}

fn invalid_preset(message: &str, action: &str) -> FormatWrightError {
    FormatWrightError::new(ErrorCode::InputInvalid, Stage::Store, message, action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(id: Uuid, name: &str) -> ConversionPreset {
        ConversionPreset {
            schema_version: PRESET_SCHEMA_VERSION,
            preset_id: id,
            name: name.to_owned(),
            target_format: "webp".to_owned(),
            quality: Some(78),
            width: Some(1920),
            dpi: None,
            color_mode: Some("rgb".to_owned()),
            video_crf: None,
            video_preset: None,
            audio_bitrate_kbps: None,
            preserve_all_streams: true,
        }
    }

    #[test]
    fn v1_library_migrates_to_v2_with_default_verb_bindings() {
        let id = Uuid::new_v4();
        let legacy = PresetLibrary {
            schema_version: 1,
            presets: vec![preset(id, "Web smaller")],
            shell_verbs: Vec::new(),
        };
        let migrated = legacy.migrate_legacy();
        assert_eq!(migrated.schema_version, PRESET_SCHEMA_VERSION);
        assert!(migrated.shell_verbs.is_empty());
        assert!(migrated.validate().is_ok());
    }

    #[test]
    fn shell_verb_bindings_validate_references_and_uniqueness() {
        let id = Uuid::new_v4();
        let mut library = PresetLibrary::empty();
        library.upsert(preset(id, "Web smaller")).expect("upsert");

        library.shell_verbs = vec![ShellVerbBinding {
            verb_id: "FormatWright.ToWebp".to_owned(),
            enabled: false,
            preset_id: Some(id),
        }];
        assert!(library.validate().is_ok());

        library.shell_verbs.push(ShellVerbBinding {
            verb_id: "FormatWright.ToWebp".to_owned(),
            enabled: true,
            preset_id: None,
        });
        assert!(
            library.validate().is_err(),
            "duplicate verb IDs are rejected"
        );

        library.shell_verbs = vec![ShellVerbBinding {
            verb_id: "FormatWright.ToPng".to_owned(),
            enabled: true,
            preset_id: Some(Uuid::new_v4()),
        }];
        assert!(
            library.validate().is_err(),
            "bindings must reference presets inside the library"
        );
    }

    #[test]
    fn upsert_is_deterministic_and_rejects_duplicate_names() {
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let mut library = PresetLibrary::empty();
        library
            .upsert(preset(first_id, " Web smaller "))
            .expect("insert preset");
        let error = library
            .upsert(preset(second_id, "web SMALLER"))
            .expect_err("case-insensitive duplicate name");
        assert_eq!(error.code, ErrorCode::InputInvalid);
        assert_eq!(library.presets.len(), 1, "failed upsert must be atomic");
        let mut updated = preset(first_id, "Web compact");
        updated.quality = Some(70);
        library.upsert(updated).expect("update by stable ID");
        assert_eq!(library.presets.len(), 1);
        assert_eq!(library.presets[0].quality, Some(70));
    }

    #[test]
    fn validates_import_bounds_and_unknown_fields() {
        let id = Uuid::new_v4();
        let invalid = format!(
            r#"{{"schema_version":1,"presets":[{{"schema_version":1,"preset_id":"{id}","name":"Bad","target_format":"WEBP","quality":78,"width":null,"dpi":null,"color_mode":null,"preserve_all_streams":true}}]}}"#
        );
        let library: PresetLibrary = serde_json::from_str(&invalid).expect("parse library");
        assert!(library.validate().is_err());
        let unknown = invalid.replace("\"name\":\"Bad\"", "\"name\":\"Bad\",\"shell\":\"rm\"");
        assert!(serde_json::from_str::<PresetLibrary>(&unknown).is_err());
    }

    #[test]
    fn merge_updates_stable_ids_without_partial_duplicates() {
        let id = Uuid::new_v4();
        let mut current = PresetLibrary::empty();
        current.upsert(preset(id, "Original")).expect("seed");
        let mut imported = PresetLibrary::empty();
        let mut update = preset(id, "Imported");
        update.target_format = "avif".to_owned();
        imported.upsert(update).expect("prepare import");
        assert_eq!(current.merge(imported).expect("merge"), 1);
        assert_eq!(current.presets[0].name, "Imported");
        assert_eq!(current.presets[0].target_format, "avif");
    }
}
