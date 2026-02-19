use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn scan_models(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create models directory: {}", dir.display()))?;
        return Ok(vec![]);
    }

    let mut models = Vec::new();
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read models directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.starts_with("ggml-") && name.ends_with(".bin")
        {
            let display_name = name
                .strip_prefix("ggml-")
                .unwrap()
                .strip_suffix(".bin")
                .unwrap()
                .to_string();
            models.push((display_name, path));
        }
    }

    models.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(models)
}

pub fn default_models_dir() -> PathBuf {
    // 1. XDG data dir: ~/.local/share/space_tts/models/
    if let Ok(home) = std::env::var("HOME") {
        let dir = PathBuf::from(home).join(".local/share/space_tts/models");
        if dir.exists() {
            return dir;
        }
    }

    // 2. Next to executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let dir = parent.join("models");
        if dir.exists() {
            return dir;
        }
        // 3. Project root (target/release/../../models)
        if let Some(project_root) = parent.parent().and_then(|p| p.parent()) {
            let dir = project_root.join("models");
            if dir.exists() {
                return dir;
            }
        }
    }

    // 4. Fallback: CWD
    PathBuf::from("models")
}

/// Resolve a model argument to an absolute path.
/// Accepts: "small", "ggml-small.bin", or a full path.
pub fn resolve_model_path(input: &str) -> PathBuf {
    let path = Path::new(input);

    // Already an existing absolute or relative path — use as-is
    if path.exists() {
        return path.to_path_buf();
    }

    let models_dir = default_models_dir();

    // Try as filename: "ggml-small.bin"
    let as_file = models_dir.join(input);
    if as_file.exists() {
        return as_file;
    }

    // Try as short name: "small" → "ggml-small.bin"
    let as_ggml = models_dir.join(format!("ggml-{input}.bin"));
    if as_ggml.exists() {
        return as_ggml;
    }

    // Nothing found — return models_dir/input so the error message is clear
    as_file
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_models_with_files() {
        let dir = std::env::temp_dir().join("space-stt-test-scan");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("ggml-base.bin"), b"fake").unwrap();
        fs::write(dir.join("ggml-tiny.bin"), b"fake").unwrap();
        fs::write(dir.join("other.bin"), b"fake").unwrap();

        let models = scan_models(&dir).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].0, "base");
        assert_eq!(models[1].0, "tiny");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_models_creates_missing_dir() {
        let dir = std::env::temp_dir().join("space-stt-test-missing");
        let _ = fs::remove_dir_all(&dir);

        let models = scan_models(&dir).unwrap();
        assert!(models.is_empty());
        assert!(dir.exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
