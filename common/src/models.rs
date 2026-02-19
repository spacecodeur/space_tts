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
    // Look for models/ directory relative to the executable, then fall back to CWD
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let dir = parent.join("models");
        if dir.exists() {
            return dir;
        }
        // Also check two levels up (target/release/../.. = project root)
        if let Some(project_root) = parent.parent().and_then(|p| p.parent()) {
            let dir = project_root.join("models");
            if dir.exists() {
                return dir;
            }
        }
    }
    PathBuf::from("models")
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
