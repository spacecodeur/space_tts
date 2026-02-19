use anyhow::Result;
use std::collections::VecDeque;
use webrtc_vad::{SampleRate, Vad, VadMode};

const FRAME_SIZE: usize = 160; // 10ms at 16kHz
const SILENCE_THRESHOLD: u32 = 50; // 500ms of silence = end of speech
const PRE_ROLL_FRAMES: usize = 5; // 50ms pre-roll buffer

pub struct VoiceDetector {
    vad: Vad,
    is_speaking: bool,
    silence_frames: u32,
    audio_buffer: Vec<i16>,
    pre_roll_buffer: VecDeque<[i16; FRAME_SIZE]>,
}

impl VoiceDetector {
    pub fn new() -> Result<Self> {
        let vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive);
        Ok(Self {
            vad,
            is_speaking: false,
            silence_frames: 0,
            audio_buffer: Vec::new(),
            pre_roll_buffer: VecDeque::with_capacity(PRE_ROLL_FRAMES),
        })
    }

    pub fn process_samples(&mut self, samples: &[i16]) -> Vec<Vec<i16>> {
        let mut segments = Vec::new();

        for chunk in samples.chunks_exact(FRAME_SIZE) {
            let frame: [i16; FRAME_SIZE] = chunk.try_into().unwrap();
            let is_voice = self.vad.is_voice_segment(&frame).unwrap_or(false);

            match (self.is_speaking, is_voice) {
                // Silence → Silence
                (false, false) => {
                    if self.pre_roll_buffer.len() >= PRE_ROLL_FRAMES {
                        self.pre_roll_buffer.pop_front();
                    }
                    self.pre_roll_buffer.push_back(frame);
                }
                // Silence → Voice
                (false, true) => {
                    self.is_speaking = true;
                    self.silence_frames = 0;
                    // Drain pre-roll into audio buffer
                    for pre_frame in self.pre_roll_buffer.drain(..) {
                        self.audio_buffer.extend_from_slice(&pre_frame);
                    }
                    self.audio_buffer.extend_from_slice(&frame);
                }
                // Voice → Voice
                (true, true) => {
                    self.silence_frames = 0;
                    self.audio_buffer.extend_from_slice(&frame);
                }
                // Voice → Silence
                (true, false) => {
                    self.audio_buffer.extend_from_slice(&frame);
                    self.silence_frames += 1;
                    if self.silence_frames >= SILENCE_THRESHOLD {
                        segments.push(std::mem::take(&mut self.audio_buffer));
                        self.is_speaking = false;
                        self.silence_frames = 0;
                        self.pre_roll_buffer.clear();
                    }
                }
            }
        }

        segments
    }

    pub fn reset(&mut self) {
        // Recreate Vad to clear its internal state (no reset API available)
        self.vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive);
        self.audio_buffer.clear();
        self.pre_roll_buffer.clear();
        self.is_speaking = false;
        self.silence_frames = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate synthetic "voice" samples (alternating high amplitude)
    /// that reliably trigger webrtc-vad voice detection.
    fn make_voice(num_frames: usize) -> Vec<i16> {
        let mut samples = Vec::with_capacity(FRAME_SIZE * num_frames);
        for i in 0..(FRAME_SIZE * num_frames) {
            // Square wave at ~500Hz (alternating every 16 samples at 16kHz)
            let val: i16 = if (i / 16) % 2 == 0 { 30000 } else { -30000 };
            samples.push(val);
        }
        samples
    }

    fn make_silence(num_frames: usize) -> Vec<i16> {
        vec![0i16; FRAME_SIZE * num_frames]
    }

    #[test]
    fn silence_produces_no_segments() {
        let mut vd = VoiceDetector::new().unwrap();
        let segments = vd.process_samples(&make_silence(100));
        assert!(segments.is_empty());
    }

    #[test]
    fn loud_then_silence_produces_segment() {
        let mut vd = VoiceDetector::new().unwrap();

        // Feed voice (50 frames = 500ms)
        let segs = vd.process_samples(&make_voice(50));
        assert!(
            segs.is_empty(),
            "Should not emit segment while still speaking"
        );

        // Feed enough silence to trigger end-of-speech
        let segs = vd.process_samples(&make_silence(SILENCE_THRESHOLD as usize + 20));
        assert_eq!(segs.len(), 1, "Should emit exactly one segment");

        // Segment should include voice frames + some pre-roll
        let seg = &segs[0];
        assert!(
            seg.len() >= FRAME_SIZE * 50,
            "Segment length {} should be >= {}",
            seg.len(),
            FRAME_SIZE * 50
        );
    }

    #[test]
    fn reset_discards_accumulated_audio() {
        let mut vd = VoiceDetector::new().unwrap();

        // Feed voice to start speaking state
        let segs = vd.process_samples(&make_voice(30));
        assert!(segs.is_empty());
        assert!(vd.is_speaking);

        // Reset clears everything including VAD internal state
        vd.reset();
        assert!(!vd.is_speaking);
        assert!(vd.audio_buffer.is_empty());
        assert!(vd.pre_roll_buffer.is_empty());

        // Feed silence — should not produce segment (VAD state is fresh)
        let segs = vd.process_samples(&make_silence(100));
        assert!(segs.is_empty());
    }

    #[test]
    fn multiple_speech_bursts() {
        let mut vd = VoiceDetector::new().unwrap();
        let mut total_segments = Vec::new();

        for _ in 0..2 {
            total_segments.extend(vd.process_samples(&make_voice(50)));
            total_segments
                .extend(vd.process_samples(&make_silence(SILENCE_THRESHOLD as usize + 20)));
        }

        assert_eq!(total_segments.len(), 2, "Should emit 2 separate segments");
    }
}
