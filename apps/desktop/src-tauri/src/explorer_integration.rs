//! Runtime Explorer verb registration (spec E-06).
//!
//! Registration responsibility moved out of the NSIS script: the installer
//! keeps only the two Open-in keys plus a `--register-shell` bootstrap call,
//! and the application owns every convert verb under HKCU from first launch
//! onward. Verb IDs are the fixed baseline set from `explorer-verbs.json`; a
//! binding can only toggle a verb or point it at a preset whose target format
//! matches the verb's target.

use std::path::Path;

use formatwright_core::{ConversionPreset, PresetLibrary, ShellVerbBinding};
use serde::{Deserialize, Serialize};

const EXPLORER_VERBS_JSON: &str = include_str!("../explorer-verbs.json");

/// One baseline entry from the built-in verb table.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerbDefinition {
    pub assoc: String,
    pub verb: String,
    pub target: String,
    pub label: String,
}

/// A verb definition resolved against the library's bindings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerbRegistration {
    pub definition: VerbDefinition,
    pub enabled: bool,
    pub preset: Option<ConversionPreset>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerbApplyReport {
    pub written: usize,
    pub removed: usize,
}

#[derive(Debug)]
pub struct VerbApplyError {
    pub verb_id: String,
    pub message: String,
}

/// Parses the built-in baseline table (fixed verb IDs; only bindings vary).
pub fn baseline_verb_table() -> Vec<VerbDefinition> {
    #[derive(Deserialize)]
    struct VerbTableFile {
        convert: Vec<VerbDefinition>,
    }
    let table: VerbTableFile =
        serde_json::from_str(EXPLORER_VERBS_JSON).expect("bundled explorer-verbs.json is valid");
    table.convert
}

/// Resolves every baseline verb against the library bindings. Missing
/// bindings mean "enabled with default parameters"; a bound preset whose
/// target does not match the verb's target is ignored rather than applied.
pub fn resolve_registrations(library: &PresetLibrary) -> Vec<VerbRegistration> {
    baseline_verb_table()
        .into_iter()
        .map(|definition| {
            let binding = library
                .shell_verbs
                .iter()
                .find(|candidate: &&ShellVerbBinding| candidate.verb_id == definition.verb);
            let preset = binding
                .and_then(|candidate| candidate.preset_id)
                .and_then(|preset_id| {
                    library
                        .presets
                        .iter()
                        .find(|preset| preset.preset_id == preset_id)
                })
                .filter(|preset| preset.target_format == definition.target)
                .cloned();
            VerbRegistration {
                enabled: binding.is_none_or(|candidate| candidate.enabled),
                definition,
                preset,
            }
        })
        .collect()
}

/// Registry key path owning one verb (HKCU-rooted, per-user install).
fn verb_key_path(registration: &VerbRegistration) -> String {
    if registration.definition.assoc == "Directory" {
        format!(
            r"Software\Classes\Directory\shell\{}",
            registration.definition.verb
        )
    } else {
        format!(
            r"Software\Classes\SystemFileAssociations\{}\shell\{}",
            registration.definition.assoc, registration.definition.verb
        )
    }
}

/// Builds the command line a verb invokes. The optional preset rides along as
/// `--preset <uuid>` and is resolved again at execution time.
fn verb_command(executable: &Path, registration: &VerbRegistration) -> String {
    use std::fmt::Write as _;

    let mut command = format!(
        "\"{}\" --shell-convert --to {}",
        executable.display(),
        registration.definition.target
    );
    if let Some(preset) = &registration.preset {
        let _ = write!(command, " --preset {}", preset.preset_id);
    }
    command.push_str(" \"%1\"");
    command
}

/// The menu label reflects the bound preset so an edited verb is visibly
/// different in Explorer ("Convert to WebP · Small WebP").
fn verb_label(registration: &VerbRegistration) -> String {
    match &registration.preset {
        Some(preset) => format!("{} · {}", registration.definition.label, preset.name),
        None => registration.definition.label.clone(),
    }
}

/// Applies every registration idempotently: enabled verbs are (re)written,
/// disabled verbs are removed. Missing parents are created by the registry.
///
/// # Errors
///
/// Returns one error per verb that could not be written or removed; remaining
/// verbs are still applied.
pub fn apply_registrations(
    executable: &Path,
    registrations: &[VerbRegistration],
) -> Result<VerbApplyReport, Vec<VerbApplyError>> {
    let mut written = 0_usize;
    let mut removed = 0_usize;
    let mut errors = Vec::new();
    for registration in registrations {
        let key_path = verb_key_path(registration);
        let outcome = if registration.enabled {
            write_verb(
                &key_path,
                &verb_label(registration),
                executable,
                &verb_command(executable, registration),
            )
        } else {
            remove_verb(&key_path)
        };
        match outcome {
            Ok(()) => {
                if registration.enabled {
                    written += 1;
                } else {
                    removed += 1;
                }
            }
            Err(message) => errors.push(VerbApplyError {
                verb_id: registration.definition.verb.clone(),
                message,
            }),
        }
    }
    if errors.is_empty() {
        Ok(VerbApplyReport { written, removed })
    } else {
        Err(errors)
    }
}

#[cfg(windows)]
mod registry {
    //! Registry access without a single line of unsafe (workspace forbids it):
    //! enabled verbs are batched into one UTF-16 `.reg` file and imported via
    //! the built-in `reg.exe`; disabled verbs are deleted with `reg delete`.
    //! Both paths take typed argv through `std::process::Command` only.

    use std::io;

    const REG_EXE: &str = "reg.exe";

    fn escape_reg_string(value: &str) -> String {
        let mut escaped = String::with_capacity(value.len());
        for character in value.chars() {
            match character {
                '\\' => escaped.push_str("\\\\"),
                '"' => escaped.push_str("\\\""),
                '\n' => escaped.push_str("\\n"),
                _ => escaped.push(character),
            }
        }
        escaped
    }

    /// Renders one `.reg` document (UTF-16LE with BOM, the only encoding
    /// `reg import` accepts for non-ASCII labels such as "·").
    fn render_reg_document(entries: &[(String, Vec<(&'static str, String)>)]) -> Vec<u8> {
        use std::fmt::Write as _;

        let mut body = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
        for (key_path, values) in entries {
            let _ = write!(body, "[{key_path}]\r\n");
            for (name, value) in values {
                let escaped = escape_reg_string(value);
                if name.is_empty() {
                    let _ = write!(body, "@=\"{escaped}\"\r\n");
                } else {
                    let _ = write!(body, "\"{name}\"=\"{escaped}\"\r\n");
                }
            }
            body.push_str("\r\n");
        }
        let units: Vec<u16> = body.encode_utf16().collect();
        let mut bytes = Vec::with_capacity(2 + units.len() * 2);
        bytes.extend_from_slice(&[0xFF, 0xFE]);
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn run_reg(arguments: &[&str]) -> io::Result<std::process::Output> {
        std::process::Command::new(REG_EXE).args(arguments).output()
    }

    pub(super) fn write_verb(
        key_path: &str,
        icon: &str,
        label: &str,
        command: &str,
    ) -> io::Result<()> {
        // .reg files require the full hive name; the HKCU short form is
        // silently ignored by reg import.
        let entries = vec![(
            format!(r"HKEY_CURRENT_USER\{key_path}"),
            vec![("MUIVerb", label.to_owned()), ("Icon", icon.to_owned())],
        )];
        let document = render_reg_document(&entries);
        write_and_import(&document)?;
        // The command sub-key goes in a second small import so its default
        // value never has to round-trip through command-line quoting.
        let command_entries = vec![(
            format!(r"HKEY_CURRENT_USER\{key_path}\command"),
            vec![("", command.to_owned())],
        )];
        write_and_import(&render_reg_document(&command_entries))
    }

    fn write_and_import(document: &[u8]) -> io::Result<()> {
        let staging = tempfile::tempdir()?;
        let file = staging.path().join("anole-verbs.reg");
        std::fs::write(&file, document)?;
        let output = run_reg(&["import", &file.to_string_lossy()])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "reg import failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    pub(super) fn delete_tree(key_path: &str) -> io::Result<()> {
        let full = format!(r"HKCU\{key_path}");
        let output = run_reg(&["delete", &full, "/f"])?;
        // reg delete on a missing key exits 1 with "unable to find"; that is
        // the desired end state for a disabled verb, not a failure.
        if output.status.success()
            || String::from_utf8_lossy(&output.stderr).contains("unable to find")
        {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "reg delete failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    #[cfg(test)]
    pub(super) fn read_value(key_path: &str, name: &str) -> io::Result<String> {
        let full = format!(r"HKCU\{key_path}");
        let mut arguments = vec!["query", &full];
        if name.is_empty() {
            arguments.push("/ve");
        } else {
            arguments.push("/v");
            arguments.push(name);
        }
        let output = run_reg(&arguments)?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "reg query failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(position) = line.find("REG_SZ") {
                let value = line[position + "REG_SZ".len()..].trim();
                return Ok(value.to_owned());
            }
        }
        Err(io::Error::other(
            "REG_SZ value not found in reg query output",
        ))
    }
}

#[cfg(windows)]
fn write_verb(key_path: &str, label: &str, executable: &Path, command: &str) -> Result<(), String> {
    let icon = format!("{},0", executable.display());
    registry::write_verb(key_path, &icon, label, command)
        .map_err(|error| format!("write verb key failed: {error}"))
}

#[cfg(windows)]
fn remove_verb(key_path: &str) -> Result<(), String> {
    registry::delete_tree(key_path).map_err(|error| format!("remove verb key failed: {error}"))
}

#[cfg(not(windows))]
fn write_verb(
    _key_path: &str,
    _label: &str,
    _executable: &Path,
    _command: &str,
) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
fn remove_verb(_key_path: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn definition(assoc: &str, verb: &str, target: &str) -> VerbDefinition {
        VerbDefinition {
            assoc: assoc.to_owned(),
            verb: verb.to_owned(),
            target: target.to_owned(),
            label: format!("Convert to {target}"),
        }
    }

    fn preset_named(name: &str, target: &str) -> ConversionPreset {
        ConversionPreset {
            schema_version: formatwright_core::PRESET_SCHEMA_VERSION,
            preset_id: Uuid::new_v4(),
            name: name.to_owned(),
            target_format: target.to_owned(),
            quality: Some(80),
            width: None,
            dpi: None,
            color_mode: None,
            video_crf: None,
            video_preset: None,
            audio_bitrate_kbps: None,
            preserve_all_streams: true,
        }
    }

    #[test]
    fn bundled_table_has_nineteen_entries_with_unique_assoc_verb_pairs() {
        let table = baseline_verb_table();
        assert_eq!(table.len(), 19, "baseline table changed; update this test");
        let mut pairs = std::collections::BTreeSet::new();
        for definition in &table {
            assert!(
                pairs.insert((definition.assoc.as_str(), definition.verb.as_str())),
                "duplicate assoc+verb pair"
            );
        }
        // Verb IDs repeat across associations (ToWebp on png/jpg/jpeg); a
        // binding keys on the verb ID and applies to every association.
        let unique_verbs: std::collections::BTreeSet<&str> = table
            .iter()
            .map(|definition| definition.verb.as_str())
            .collect();
        assert!(table.len() > unique_verbs.len());
    }

    #[test]
    fn missing_bindings_default_to_enabled_without_preset() {
        let mut library = PresetLibrary::empty();
        let small = preset_named("Small WebP", "webp");
        library.upsert(small.clone()).expect("upsert");
        // A binding pointing at a preset with a DIFFERENT target is ignored.
        library.shell_verbs = vec![ShellVerbBinding {
            verb_id: "FormatWright.ToWebp".to_owned(),
            enabled: false,
            preset_id: None,
        }];
        let registrations = resolve_registrations(&library);
        let to_webp = registrations
            .iter()
            .find(|r| r.definition.verb == "FormatWright.ToWebp")
            .expect("webp verb");
        assert!(!to_webp.enabled);
        assert!(to_webp.preset.is_none());
        let to_png = registrations
            .iter()
            .find(|r| r.definition.verb == "FormatWright.ToPng")
            .expect("png verb");
        assert!(to_png.enabled);
        assert!(to_png.preset.is_none());
    }

    #[test]
    fn cross_target_preset_binding_is_ignored() {
        let mut library = PresetLibrary::empty();
        let jpg_preset = preset_named("Small JPG", "jpg");
        library.upsert(jpg_preset).expect("upsert");
        // Bind the png verb (target png) to a jpg preset: must not apply.
        let preset_id = library.presets[0].preset_id;
        library.shell_verbs = vec![ShellVerbBinding {
            verb_id: "FormatWright.ToPng".to_owned(),
            enabled: true,
            preset_id: Some(preset_id),
        }];
        let registrations = resolve_registrations(&library);
        let to_png = registrations
            .iter()
            .find(|r| r.definition.verb == "FormatWright.ToPng")
            .expect("png verb");
        assert!(to_png.preset.is_none(), "cross-target preset must not bind");
    }

    #[test]
    fn verb_command_and_label_carry_the_bound_preset() {
        let preset = preset_named("Small WebP", "webp");
        let registration = VerbRegistration {
            definition: definition(".png", "FormatWright.ToWebp", "webp"),
            enabled: true,
            preset: Some(preset.clone()),
        };
        let command = verb_command(Path::new(r"C:\Apps\Anole.exe"), &registration);
        assert!(command.contains("--shell-convert --to webp"));
        assert!(command.contains(&format!("--preset {}", preset.preset_id)));
        assert!(command.ends_with("\"%1\""));
        assert_eq!(verb_label(&registration), "Convert to webp · Small WebP");
    }

    #[cfg(windows)]
    #[test]
    fn apply_writes_enabled_and_removes_disabled_verbs_in_hkcu() {
        // Scratch association under the app's own verb namespace; cleaned on
        // exit so the test never leaves user-visible menu entries behind.
        let scratch_assoc = ".fw-verb-test";
        let enabled = VerbRegistration {
            definition: definition(scratch_assoc, "FormatWright.TestOn", "png"),
            enabled: true,
            preset: None,
        };
        let disabled = VerbRegistration {
            definition: definition(scratch_assoc, "FormatWright.TestOff", "png"),
            enabled: false,
            preset: None,
        };

        // Pre-create the disabled verb so removal has real work to do.
        let disabled_path = verb_key_path(&disabled);
        registry::write_verb(
            &disabled_path,
            "unused,0",
            "stale",
            r#""unused" --shell-convert --to png "%1""#,
        )
        .expect("seed stale verb");

        let executable = std::env::current_exe().expect("test exe");
        let report = apply_registrations(&executable, &[enabled.clone(), disabled.clone()])
            .expect("apply registrations");
        assert_eq!(report.written, 1);
        assert_eq!(report.removed, 1);

        let enabled_path = verb_key_path(&enabled);
        let label = registry::read_value(&enabled_path, "MUIVerb").expect("label readable");
        assert_eq!(label, "Convert to png");
        let command = registry::read_value(&format!(r"{enabled_path}\command"), "")
            .expect("command readable");
        assert!(command.starts_with(&format!("\"{}\"", executable.display())));
        assert!(command.contains("--shell-convert --to png \"%1\""));
        assert!(
            registry::read_value(&disabled_path, "MUIVerb").is_err(),
            "disabled verb removed"
        );

        // Cleanup: remove both scratch keys.
        let _ = registry::delete_tree(&enabled_path);
        let _ = registry::delete_tree(&disabled_path);
    }
}
