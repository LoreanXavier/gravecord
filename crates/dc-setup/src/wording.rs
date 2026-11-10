//! Words shown in the setup window that depend on data: the version line and the progress step.

/// The version to show, or `None` for an empty one and for 0.0.0 (a development build has no real number).
pub fn version_label(version: &str) -> Option<String> {
    let version = version.trim().trim_start_matches(['v', 'V']);
    let core = version.split(['-', '+']).next().unwrap_or_default();
    if core.is_empty() || core.split('.').all(|part| part == "0") {
        None
    } else {
        Some(version.to_owned())
    }
}

/// The step in plain words for a label the installer reports.
pub fn step_words(label: &str) -> &str {
    match label {
        "Copying files" | "Replacing files" => "Copying files",
        "Adding shortcuts" => "Creating shortcuts",
        "Registering the app" | "Done" => "Finishing",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_or_empty_version_is_hidden() {
        for hidden in [
            "",
            "  ",
            "0.0.0",
            "v0.0.0",
            "0.0.0-dev",
            "0.0",
            "0.0.0+build5",
        ] {
            assert_eq!(version_label(hidden), None, "{hidden:?}");
        }
    }

    #[test]
    fn a_real_version_is_shown() {
        assert_eq!(version_label("1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(version_label("0.1.0").as_deref(), Some("0.1.0"));
        assert_eq!(version_label("v0.0.1").as_deref(), Some("0.0.1"));
        assert_eq!(
            version_label("1.0.0-beta.2").as_deref(),
            Some("1.0.0-beta.2")
        );
    }

    #[test]
    fn installer_labels_become_plain_steps() {
        assert_eq!(step_words("Copying files"), "Copying files");
        assert_eq!(step_words("Replacing files"), "Copying files");
        assert_eq!(step_words("Adding shortcuts"), "Creating shortcuts");
        assert_eq!(step_words("Registering the app"), "Finishing");
        assert_eq!(step_words("Done"), "Finishing");
        assert_eq!(step_words("Preparing"), "Preparing");
    }
}
