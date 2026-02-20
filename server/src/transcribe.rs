use anyhow::Result;

use space_tts_common::warn;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
    convert_integer_to_float_audio,
};

pub trait Transcriber: Send {
    fn transcribe(&mut self, audio_i16: &[i16]) -> Result<String>;
}

pub struct LocalTranscriber {
    state: WhisperState,
    language: String,
}

impl LocalTranscriber {
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
}

impl Transcriber for LocalTranscriber {
    fn transcribe(&mut self, audio_i16: &[i16]) -> Result<String> {
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
        params.set_no_speech_thold(0.6);
        // Initial prompt helps Whisper stay in the target language and use proper vocabulary
        params.set_initial_prompt(initial_prompt(&self.language));

        if let Err(e) = self.state.full(params, &audio_f32) {
            warn!("Transcription error: {e}");
            return Ok(String::new());
        }

        let mut text = String::new();
        for segment in self.state.as_iter() {
            match segment.to_str_lossy() {
                Ok(s) => text.push_str(&s),
                Err(e) => warn!("Segment text error: {e}"),
            }
        }

        let text = text.trim().to_string();
        Ok(filter_hallucinations(&text))
    }
}

/// Filter out common Whisper hallucinations (YouTube subtitle artifacts).
/// Returns empty string if the entire text is a hallucination.
fn filter_hallucinations(text: &str) -> String {
    // Long, specific patterns — safe to match anywhere (trailing match)
    const TRAILING_HALLUCINATIONS: &[&str] = &[
        "merci d'avoir regardé",
        "merci d'avoir regardé la vidéo",
        "merci d'avoir regardé cette vidéo",
        "merci de votre attention",
        "sous-titres réalisés par",
        "sous-titrage société radio-canada",
        "like and subscribe",
        "please subscribe",
        "thanks for watching",
        "thank you for watching",
    ];

    // Short/generic patterns — only discard if they are the ENTIRE output
    const FULLMATCH_HALLUCINATIONS: &[&str] = &[
        "sous-titres par",
        "sous-titrage st'",
        "sous-titrage",
        "société radio-canada",
        "subscribe",
        "merci",
    ];

    if is_repetitive(&text.to_lowercase()) {
        return String::new();
    }

    let lower = text.to_lowercase();
    let stripped = lower.trim_end_matches(['.', '!', '?', ' ', ',']);

    // Full-match check: both lists
    for pattern in TRAILING_HALLUCINATIONS.iter().chain(FULLMATCH_HALLUCINATIONS.iter()) {
        if stripped == *pattern {
            return String::new();
        }
    }

    // Trailing match: only long specific patterns
    let mut result = text.to_string();
    for pattern in TRAILING_HALLUCINATIONS {
        if let Some(pos) = lower.find(pattern) {
            result.truncate(pos);
        }
    }

    // Strip trailing lone "Merci !" / "Merci!" often appended
    let trimmed = result.trim().trim_end_matches('!').trim();
    if trimmed.ends_with("Merci") || trimmed.ends_with("merci") {
        if let Some(pos) = result.to_lowercase().rfind("merci") {
            let before = &result[..pos];
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

    let result = result.trim().trim_end_matches(['.', '!', '?', ',']).trim().to_string();

    // Re-check: what remains after truncation may itself be a hallucination
    if result.is_empty() {
        return String::new();
    }
    let remaining = result.to_lowercase();
    let remaining_stripped = remaining.trim_end_matches(['.', '!', '?', ' ', ',']);
    for pattern in FULLMATCH_HALLUCINATIONS {
        if remaining_stripped == *pattern {
            return String::new();
        }
    }
    if is_repetitive(&remaining) {
        return String::new();
    }

    result
}

/// Detect text that is just the same word or short phrase repeated.
/// Catches "MerciMerciMerci", "merci merci merci", "thank you. thank you. thank you." etc.
fn is_repetitive(text: &str) -> bool {
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.is_empty() {
        return true;
    }

    // Check if the entire string (without spaces) is one short word repeated 3+ times
    // e.g. "mercimercimerci" = "merci" × 3
    let joined: String = words.join("");
    for len in 1..=joined.len().min(12) {
        if joined.len() % len != 0 {
            continue;
        }
        let repeats = joined.len() / len;
        if repeats >= 3 && joined == joined[..len].repeat(repeats) {
            return true;
        }
    }

    // Check if the same word appears 3+ times in a row
    // e.g. "merci merci merci"
    if words.len() >= 3 {
        let mut run = 1;
        for i in 1..words.len() {
            if words[i] == words[i - 1] {
                run += 1;
                if run >= 3 {
                    return true;
                }
            } else {
                run = 1;
            }
        }
    }

    // Check if the same 2-3 word phrase repeats 3+ times
    // e.g. "thank you thank you thank you"
    for phrase_len in 2..=3 {
        if words.len() >= phrase_len * 3 {
            let phrase = &words[..phrase_len];
            let repeats = words.chunks(phrase_len).take_while(|c| *c == phrase).count();
            if repeats >= 3 {
                return true;
            }
        }
    }

    false
}

fn initial_prompt(language: &str) -> &'static str {
    match language {
        "fr" => "Bonjour, ceci est une transcription en français.",
        "de" => "Hallo, dies ist eine Transkription auf Deutsch.",
        "es" => "Hola, esta es una transcripción en español.",
        "it" => "Ciao, questa è una trascrizione in italiano.",
        "pt" => "Olá, esta é uma transcrição em português.",
        "ja" => "こんにちは、これは日本語の文字起こしです。",
        "zh" => "你好，这是中文转录。",
        _ => "Hello, this is an English transcription.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_full_hallucination() {
        assert_eq!(filter_hallucinations("Merci d'avoir regardé la vidéo!"), "");
        assert_eq!(filter_hallucinations("Merci d'avoir regardé."), "");
        assert_eq!(filter_hallucinations("Thanks for watching"), "");
        assert_eq!(filter_hallucinations("Sous-titrage Société Radio-Canada"), "");
        assert_eq!(filter_hallucinations("Sous-titrage"), "");
        assert_eq!(filter_hallucinations("Subscribe"), "");
    }

    #[test]
    fn filter_repetitive_hallucination() {
        assert_eq!(filter_hallucinations("MerciMerciMerci"), "");
        assert_eq!(filter_hallucinations("merci merci merci"), "");
        assert_eq!(filter_hallucinations("Thank you. Thank you. Thank you."), "");
        assert_eq!(filter_hallucinations("you you you you"), "");
    }

    #[test]
    fn filter_trailing_hallucination() {
        assert_eq!(
            filter_hallucinations("Bonjour tout le monde. Merci d'avoir regardé la vidéo!"),
            "Bonjour tout le monde"
        );
        assert_eq!(
            filter_hallucinations("Bonjour. Sous-titrage Société Radio-Canada"),
            "Bonjour"
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
        // Short patterns used in real speech must NOT be stripped mid-sentence
        assert_eq!(
            filter_hallucinations("Je veux activer le sous-titrage automatique"),
            "Je veux activer le sous-titrage automatique"
        );
        assert_eq!(
            filter_hallucinations("I need to subscribe to the service"),
            "I need to subscribe to the service"
        );
    }
}
