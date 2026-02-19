use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
    convert_integer_to_float_audio,
};

pub struct Transcriber {
    state: WhisperState,
    language: String,
}

impl Transcriber {
    pub fn new(model_path: &str, language: &str) -> Result<Self> {
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::new())
            .map_err(|e| anyhow::anyhow!("Failed to load whisper model: {e}"))?;
        let state = ctx
            .create_state()
            .map_err(|e| anyhow::anyhow!("Failed to create whisper state: {e}"))?;
        Ok(Self {
            state,
            language: language.to_string(),
        })
    }

    pub fn transcribe(&mut self, audio_i16: &[i16]) -> Result<String> {
        // Convert i16 to f32
        let mut audio_f32 = vec![0.0f32; audio_i16.len()];
        convert_integer_to_float_audio(audio_i16, &mut audio_f32)
            .map_err(|e| anyhow::anyhow!("Audio conversion failed: {e}"))?;

        let mut params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: -1.0,
        });
        params.set_language(Some(&self.language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_nst(true);
        params.set_no_speech_thold(0.4);
        // Initial prompt helps Whisper stay in the target language and use proper vocabulary
        params.set_initial_prompt("Bonjour, ceci est une transcription en français.");

        if let Err(e) = self.state.full(params, &audio_f32) {
            eprintln!("Transcription error: {e}");
            return Ok(String::new());
        }

        let mut text = String::new();
        for segment in self.state.as_iter() {
            match segment.to_str_lossy() {
                Ok(s) => text.push_str(&s),
                Err(e) => eprintln!("Segment text error: {e}"),
            }
        }

        let text = text.trim().to_string();
        Ok(filter_hallucinations(&text))
    }
}

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
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("ggml-") && name.ends_with(".bin") {
                let display_name = name
                    .strip_prefix("ggml-")
                    .unwrap()
                    .strip_suffix(".bin")
                    .unwrap()
                    .to_string();
                models.push((display_name, path));
            }
        }
    }

    models.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(models)
}

/// Filter out common Whisper hallucinations (YouTube subtitle artifacts).
/// Returns empty string if the entire text is a hallucination.
fn filter_hallucinations(text: &str) -> String {
    const HALLUCINATIONS: &[&str] = &[
        "merci d'avoir regardé",
        "merci d'avoir regardé la vidéo",
        "merci d'avoir regardé cette vidéo",
        "merci de votre attention",
        "sous-titres réalisés par",
        "sous-titres par",
        "sous-titrage st'",
        "thanks for watching",
        "thank you for watching",
        "subscribe",
        "like and subscribe",
        "please subscribe",
    ];

    let lower = text.to_lowercase();

    // If the entire text is a hallucination, discard it
    for pattern in HALLUCINATIONS {
        if lower.trim_end_matches(['.', '!', '?', ' ']) == *pattern {
            return String::new();
        }
    }

    // Strip trailing hallucination appended after real speech
    let mut result = text.to_string();
    for pattern in HALLUCINATIONS {
        if let Some(pos) = lower.find(pattern) {
            result.truncate(pos);
        }
    }

    // Also strip trailing lone "Merci !" / "Merci!" often appended
    let trimmed = result.trim().trim_end_matches('!').trim();
    if trimmed.ends_with("Merci") || trimmed.ends_with("merci") {
        // Only strip if "Merci" is at the very end and preceded by space/punctuation
        if let Some(pos) = result.to_lowercase().rfind("merci") {
            let before = &result[..pos];
            // Strip only if preceded by punctuation or space (not part of a real word)
            if before.is_empty()
                || before.ends_with(' ')
                || before.ends_with('.')
                || before.ends_with(',')
                || before.ends_with('!')
                || before.ends_with('?')
            {
                result.truncate(pos);
            }
        }
    }

    result.trim().trim_end_matches(['.', '!', '?', ',']).trim().to_string()
}

pub fn default_models_dir() -> PathBuf {
    // Look for models/ directory relative to the executable, then fall back to CWD
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
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
    fn filter_full_hallucination() {
        assert_eq!(filter_hallucinations("Merci d'avoir regardé la vidéo!"), "");
        assert_eq!(filter_hallucinations("Merci d'avoir regardé."), "");
        assert_eq!(filter_hallucinations("Thanks for watching"), "");
    }

    #[test]
    fn filter_trailing_hallucination() {
        assert_eq!(
            filter_hallucinations("Bonjour tout le monde. Merci d'avoir regardé la vidéo!"),
            "Bonjour tout le monde"
        );
    }

    #[test]
    fn filter_trailing_merci() {
        assert_eq!(
            filter_hallucinations("Il fait beau aujourd'hui. Merci!"),
            "Il fait beau aujourd'hui"
        );
        assert_eq!(
            filter_hallucinations("Il fait beau aujourd'hui. Merci !"),
            "Il fait beau aujourd'hui"
        );
    }

    #[test]
    fn filter_keeps_real_text() {
        assert_eq!(
            filter_hallucinations("Bonjour, je suis Matthieu"),
            "Bonjour, je suis Matthieu"
        );
        // "merci" as part of real speech should be kept
        assert_eq!(
            filter_hallucinations("Je te remercie pour ton aide"),
            "Je te remercie pour ton aide"
        );
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
