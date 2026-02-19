---
title: 'Space STT — Local Speech-to-Text Terminal Injector'
slug: 'space-stt'
created: '2026-02-19'
status: 'ready-for-dev'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['rust', 'whisper-rs 0.15+', 'whisper.cpp', 'cpal 0.17+', 'webrtc-vad 0.4', 'rubato', 'dotool', 'evdev', 'ratatui']
files_to_modify: ['Cargo.toml', 'src/main.rs', 'src/audio.rs', 'src/vad.rs', 'src/transcribe.rs', 'src/inject.rs', 'src/hotkey.rs', 'src/tui.rs']
code_patterns: ['async channels (crossbeam/mpsc) for audio pipeline', 'chunk-based whisper transcription', 'evdev blocking event loop on dedicated thread', 'ratatui immediate-mode TUI', 'audio resampling via rubato']
test_patterns: ['unit tests per module', 'integration test with mock audio input']
---

# Tech-Spec: Space STT — Local Speech-to-Text Terminal Injector

**Created:** 2026-02-19

## Overview

### Problem Statement

Using CLI tools like Claude Code requires constant keyboard typing, which is slow and limits productivity. The user wants to interact with any terminal application purely by voice — speak naturally and have the transcribed text injected into whichever terminal window currently has focus.

### Solution

A single Rust binary that captures microphone audio, detects voice activity, transcribes speech to text locally using whisper.cpp (via whisper-rs with CUDA GPU acceleration), and injects the resulting text into the focused window using dotool — acting as a transparent virtual keyboard. A push-to-talk hotkey (via evdev) controls when listening is active. A TUI menu at startup allows selecting the audio device, Whisper model, and hotkey.

### Scope

**In Scope:**
- TUI startup menu (ratatui): select audio device, Whisper model, push-to-talk hotkey
- Push-to-talk hotkey via evdev (global, works under Wayland)
- Continuous microphone audio capture via cpal (at device native sample rate)
- Audio resampling to 16kHz via rubato (supports any device sample rate)
- Voice Activity Detection via webrtc-vad to detect speech boundaries
- Local STT transcription via whisper-rs (whisper.cpp) with CUDA/GPU support
- Text injection into the focused window via dotool (Unicode/French accents supported)
- Support for multiple Whisper model sizes (large for desktop Nvidia 4080 16GB, tiny/base for modest laptop)
- System prerequisites documentation for Fedora (dnf) and Debian/Ubuntu (apt)
- Single Rust binary

**Out of Scope:**
- TTS (text-to-speech / vocal feedback) — future consideration
- External APIs / cloud services
- X11-specific support (dotool + evdev work on both Wayland and X11 via uinput/kernel)
- GUI / graphical interface
- Hot-swap audio device during runtime

## Context for Development

### Codebase Patterns

Greenfield project — Confirmed Clean Slate. Architecture to establish:

- **Modular file structure**: one module per concern (audio, vad, transcribe, inject, hotkey, tui)
- **Async pipeline with channels**: cpal audio thread → crossbeam channel → resampling → VAD processing → whisper transcription → dotool injection
- **Dedicated threads**: audio capture thread (cpal callback, high-priority), evdev hotkey listener thread, main thread for TUI then pipeline orchestration
- **Chunk-based transcription**: whisper-rs processes complete audio buffers, not streaming. Accumulate voiced audio until silence detected, then transcribe the full segment.
- **Error handling pattern**: all modules return `anyhow::Result`. Errors are classified as **fatal** (app must exit) or **non-fatal** (log and continue):
  - **Fatal errors** (propagate via `?`, clean exit with message): model file not found or load failure, no audio input devices available, `/dev/uinput` not accessible, dotool not installed, audio device doesn't produce samples (stream creation failure), TUI setup cancelled by user.
  - **Non-fatal errors** (log to stderr, continue): transcription failure on a segment (skip it), dotool process crash (respawn), evdev device unplugged (disable PTT, log), channel full (drop segment, log warning), audio chunk dropped by cpal (silent, expected under load).
  - A dev agent must use `?` for fatal errors and `eprintln!` + continue for non-fatal errors. Never `unwrap()` or `expect()` in pipeline code — always handle explicitly.
- **Logging pattern**: use `eprintln!` for all status and error output to stderr. No logging framework for MVP — keep it simple. All user-facing messages go to stderr to avoid interfering with dotool text injection.

### Files to Create

| File | Purpose |
| ---- | ------- |
| `Cargo.toml` | Project manifest with dependencies and feature flags |
| `src/main.rs` | Entry point: TUI startup → pipeline orchestration → graceful shutdown |
| `src/audio.rs` | cpal device enumeration, audio capture stream, and resampling to 16kHz |
| `src/vad.rs` | WebRTC VAD wrapper: voice/silence detection on audio frames with pre-roll buffer |
| `src/transcribe.rs` | whisper-rs context loading and chunk transcription |
| `src/inject.rs` | dotool text injection via stdin pipe with text sanitization and preflight checks |
| `src/hotkey.rs` | evdev global hotkey listener (push-to-talk set/clear) |
| `src/tui.rs` | ratatui startup menus (device, model, hotkey selection) |

### Technical Decisions

- **Language:** Rust — single binary, performance-critical audio processing
- **STT Engine:** whisper.cpp via `whisper-rs` 0.15+ (Codeberg, actively maintained) — CUDA via feature flag, audio input must be f32 16kHz mono
- **Audio Capture:** `cpal` 0.17+ — ALSA backend, transparent PipeWire routing on Fedora. Capture at device's preferred sample rate (typically 48kHz on PipeWire).
- **Audio Resampling:** `rubato` — resample from device native rate to 16kHz. Handles any source rate. Uses `SincFixedIn` resampler for quality.
- **VAD:** `webrtc-vad` 0.4 — Google WebRTC VAD bindings. Frames of 10ms (160 samples at 16kHz). Modes: Quality/LowBitrate/Aggressive/VeryAggressive. Note: crate unmaintained since 2019 but functional. Upgrade path: Silero VAD via `ort` (ONNX Runtime) if needed.
- **Text Injection:** `dotool` — uinput-based, Unicode support via XKB, works on GNOME Wayland + KDE. No daemon required for basic use. Invoked by piping commands to dotool stdin. Text is sanitized before injection (newlines replaced, control characters stripped).
- **Global Hotkey:** `evdev` crate — reads `/dev/input/eventX` directly. User must be in `input` group. Works under Wayland.
- **TUI:** `ratatui` — standard Rust TUI. Immediate-mode rendering. `ListState` for selection menus.
- **Platform:** Linux (Fedora primary, Debian/Ubuntu secondary), Wayland (GNOME), also works on X11 via uinput.
- **GPU:** Nvidia 4080 16GB (desktop) via CUDA. Fallback: CPU with smaller models (tiny/base) for laptop.

### Key Technical Constraints

1. **whisper-rs is not `Send`** — WhisperContext must stay on one thread. Transcription runs on a dedicated thread, audio segments sent via channel.
2. **cpal callback is high-priority** — never block in the audio callback. Use lock-free channel to forward samples.
3. **webrtc-vad is not `Send`/`Sync`** — must live on the same thread as the audio processing pipeline.
4. **cpal::Device may be `!Send` on some backends** — TUI and pipeline both run on the main thread sequentially (TUI completes first, then pipeline starts), so `SetupConfig` never crosses a thread boundary. Do NOT move `cpal::Device` to another thread.
5. **dotool needs uinput access** — user must be in the `input` group AND `/dev/uinput` must be accessible. These are two distinct permissions: evdev reads `/dev/input/eventX`, dotool writes to `/dev/uinput`. Both require `input` group on most distros but the uinput device may have separate permissions. A preflight check at startup verifies both.
6. **CUDA build requires CMake + CUDA Toolkit** — documented in prerequisites.
7. **Whisper models not bundled** — must be downloaded separately from HuggingFace (~75MB to ~3.1GB).
8. **Audio format chain**: cpal captures i16 at native rate (e.g. 48kHz) → `rubato` resamples to 16kHz i16 mono → VAD consumes i16 frames → whisper needs f32 (use `whisper_rs::convert_integer_to_float_audio`).

### System Prerequisites

#### Fedora (dnf)

```bash
# Build dependencies
sudo dnf install cmake gcc gcc-c++ pkg-config alsa-lib-devel

# dotool (build from source)
sudo dnf install golang libxkbcommon-devel scdoc
git clone https://git.sr.ht/~geb/dotool && cd dotool
./build.sh && sudo ./build.sh install

# Permissions (needed for both evdev hotkey AND dotool uinput)
sudo usermod -aG input $USER
# Log out and back in

# CUDA (for GPU acceleration — optional, desktop only)
sudo dnf install cuda-toolkit
# Ensure nvcc is in PATH

# Whisper models (download desired size)
mkdir -p ~/.local/share/space-stt/models
wget -P ~/.local/share/space-stt/models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin
```

#### Debian / Ubuntu (apt)

```bash
# Build dependencies
sudo apt install cmake gcc g++ pkg-config libasound2-dev

# dotool (build from source)
sudo apt install golang libxkbcommon-dev scdoc
git clone https://git.sr.ht/~geb/dotool && cd dotool
./build.sh && sudo ./build.sh install

# Permissions (needed for both evdev hotkey AND dotool uinput)
sudo usermod -aG input $USER
# Log out and back in

# CUDA (for GPU acceleration — optional, desktop only)
# Follow NVIDIA CUDA Toolkit installation for your Ubuntu version
# https://developer.nvidia.com/cuda-downloads

# Whisper models (download desired size)
mkdir -p ~/.local/share/space-stt/models
wget -P ~/.local/share/space-stt/models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin
```

### Available Whisper Models

| Model | Size | VRAM | Use Case |
|-------|------|------|----------|
| `ggml-tiny.bin` | ~75 MB | ~1 GB | Laptop CPU, fastest, lowest quality |
| `ggml-base.bin` | ~142 MB | ~1 GB | Laptop CPU/GPU, good balance |
| `ggml-small.bin` | ~466 MB | ~2 GB | Mid-range GPU |
| `ggml-medium.bin` | ~1.5 GB | ~5 GB | Strong GPU |
| `ggml-large-v3.bin` | ~3.1 GB | ~10 GB | Desktop 4080, best quality |

## Implementation Plan

### Tasks

- [ ] **Task 1: Project scaffolding**
  - File: `Cargo.toml`
  - Action: Initialize Rust project with `cargo init --name space-stt`. Configure `Cargo.toml` with all dependencies and a `cuda` feature flag:
    ```toml
    [features]
    default = []
    cuda = ["whisper-rs/cuda"]

    [dependencies]
    whisper-rs = "0.15"
    cpal = "0.17"
    webrtc-vad = "0.4"
    rubato = "0.16"
    evdev = "0.12"
    ratatui = "0.29"
    crossterm = "0.28"
    crossbeam-channel = "0.5"
    ctrlc = { version = "3", features = ["termination"] }
    anyhow = "1"
    ```
  - File: `src/main.rs`
  - Action: Create skeleton with module declarations (`mod audio; mod vad; mod transcribe; mod inject; mod hotkey; mod tui;`) and a placeholder `fn main()` that prints a startup message. Verify the project compiles with `cargo check`.
  - Notes: CUDA is opt-in via `cargo build --features cuda`. Default build is CPU-only for laptop compatibility. `ctrlc` crate with `termination` feature handles Ctrl+C gracefully. `whisper-rs` pulls in its own build dependencies (`cc`, `cmake`) transitively via its `build.rs` — no explicit `[build-dependencies]` needed in our Cargo.toml.

- [ ] **Task 2: Text injection module**
  - File: `src/inject.rs`
  - Action: Create module with:
    - `pub trait TextInjector { fn type_text(&mut self, text: &str) -> Result<()>; }` — abstraction over text injection backend. Allows future swap to `wtype`, `eitype`, or clipboard-paste without changing the pipeline.
    - `pub struct Injector` that holds a `std::process::Child` (persistent dotool process with stdin pipe open) and the XKB layout string. Implements `TextInjector`.
    - `Injector::new(xkb_layout: &str) -> Result<Self>`:
      1. **Preflight check**: verify `/dev/uinput` exists and is readable/writable by the current user. If not, return error: `"Cannot access /dev/uinput. Ensure your user is in the 'input' group and log out/in."`.
      2. Check that `dotool` is in PATH (e.g. `which dotool`). If not, return error: `"dotool not found. Install it: https://git.sr.ht/~geb/dotool"`.
      3. Spawn `dotool` process with stdin piped. Set `DOTOOL_XKB_LAYOUT` to the provided `xkb_layout` value.
    - `fn sanitize(text: &str) -> String`: replace all newline characters (`\n`, `\r\n`, `\r`) with spaces. Remove null bytes (`\0`). Remove all Unicode control characters in ranges U+0000–U+001F (except U+0020 space) and U+007F–U+009F. Strip leading/trailing whitespace. This prevents dotool command stream corruption.
    - `Injector::type_text(&mut self, text: &str) -> Result<()>`: sanitize the text first. If the sanitized result is empty, return Ok (no-op). Write `type {sanitized}\n` to dotool's stdin. Flush after each write. If the write fails (broken pipe = dotool crashed), attempt to respawn the dotool process once. If respawn also fails, return the error.
    - `impl Drop for Injector`: call `child.kill()` then `child.wait()` to reap the process and avoid zombies.
  - Notes: dotool reads one command per line from stdin. Keeping a persistent process avoids the uinput device registration delay on each invocation. Sanitization is critical — Whisper can emit newlines in multi-sentence transcriptions, and any control character could corrupt the dotool command stream. The `wait()` after `kill()` in Drop is mandatory to prevent zombie processes.

- [ ] **Task 3: Audio capture module**
  - File: `src/audio.rs`
  - Action: Create module with:
    - `pub fn list_input_devices() -> Result<Vec<(String, cpal::Device)>>`: enumerate all input devices via `cpal::default_host().input_devices()`, return vec of `(device_name, device)`.
    - `pub struct CaptureConfig { pub sample_rate: u32, pub channels: u16 }` — describes the actual device capture format, needed by the resampler.
    - `pub fn start_capture(device: &cpal::Device, sender: crossbeam_channel::Sender<Vec<i16>>) -> Result<(cpal::Stream, CaptureConfig)>`: query the device's preferred/default input config via `device.default_input_config()`. Use the device's native sample rate and channel count. Build an input stream with `i16` format. In the callback, clone the sample slice into a `Vec<i16>` and send through the channel via `try_send` (never block; drop samples if channel full). Return both the stream and the `CaptureConfig` so the caller knows the actual sample rate for resampling.
    - `pub fn create_resampler(source_rate: u32, target_rate: u32, channels: u16) -> Result<impl FnMut(&[i16]) -> Vec<i16>>`: create a `rubato::SincFixedIn` resampler configured for the given rates. Return a closure that accepts a chunk of i16 samples at the source rate and returns resampled i16 samples at the target rate. If the source and target rates are equal, return a no-op closure (just clone the input). Handle mono conversion: if channels > 1, average channels to mono before resampling.
  - Notes: The returned `cpal::Stream` must be kept alive by the caller (dropping it stops capture). By accepting the device's native rate and resampling, we avoid the "device doesn't support 16kHz" problem that would crash the app on most PipeWire setups.

- [ ] **Task 4: VAD module**
  - File: `src/vad.rs`
  - Action: Create module with:
    - `pub struct VoiceDetector` wrapping `webrtc_vad::Vad` and state for tracking speech segments.
    - Internal state: `is_speaking: bool`, `silence_frames: u32`, `audio_buffer: Vec<i16>`, `pre_roll_buffer: VecDeque<[i16; FRAME_SIZE]>`.
    - Constants: `FRAME_SIZE: usize = 160` (10ms at 16kHz), `SILENCE_THRESHOLD: u32 = 50` (500ms of silence = end of speech — tuned for dictation use case where the user pauses to think between words/sentences), `PRE_ROLL_FRAMES: usize = 5` (50ms of audio before voice onset, to avoid clipping leading phonemes).
    - `VoiceDetector::new() -> Result<Self>`: create `Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive)`. Initialize `pre_roll_buffer` as empty `VecDeque` with capacity `PRE_ROLL_FRAMES`.
    - `VoiceDetector::process_samples(&mut self, samples: &[i16]) -> Vec<Vec<i16>>`: process incoming audio samples (must already be 16kHz mono) in 160-sample frames. For each frame, call `vad.is_voice_segment()`. Track state transitions:
      - **Silence → Silence**: push frame into `pre_roll_buffer` (circular, drop oldest if at capacity). No-op otherwise.
      - **Silence → Voice**: set `is_speaking = true`. Drain `pre_roll_buffer` into `audio_buffer` first (this preserves audio just before speech onset), then append current frame to `audio_buffer`.
      - **Voice → Voice**: append frame to `audio_buffer`. Reset `silence_frames = 0`.
      - **Voice → Silence**: append frame to `audio_buffer`. Increment `silence_frames`. If `silence_frames >= SILENCE_THRESHOLD`, emit the completed `audio_buffer` as a finished segment, reset `is_speaking = false`, clear `audio_buffer` and `pre_roll_buffer`.
    - `VoiceDetector::reset(&mut self)`: clear `audio_buffer`, `pre_roll_buffer`, set `is_speaking = false`, `silence_frames = 0`. Called when push-to-talk is released to discard any incomplete speech segment. This is intentional — the user must finish their sentence before releasing the key.
    - Return value of `process_samples`: a `Vec` of completed speech segments (may be empty, may contain multiple segments if the input was large).
  - Notes: `VoiceDetector` is `!Send` because `Vad` is `!Send`. It must live on the same thread that calls `process_samples`. The `SILENCE_THRESHOLD` of 50 frames (500ms) is tuned for dictation — the user pauses to think between words when dictating to Claude, so a shorter threshold would cut sentences prematurely. The `PRE_ROLL_FRAMES` of 5 (50ms) prevents clipping the start of words. The `reset()` method is critical for the PTT-release case: incomplete segments are intentionally discarded (user chose this behavior for simplicity).

- [ ] **Task 5: Transcription module**
  - File: `src/transcribe.rs`
  - Action: Create module with:
    - `pub struct Transcriber` wrapping `WhisperContext`, `WhisperState`, and the configured `language: String`.
    - `Transcriber::new(model_path: &str, language: &str) -> Result<Self>`: load model via `WhisperContext::new_with_params(model_path, WhisperContextParameters::default())`, then `ctx.create_state()`. Store `language` for use in `transcribe()`.
    - `Transcriber::transcribe(&mut self, audio_i16: &[i16]) -> Result<String>`: convert i16 to f32 via `whisper_rs::convert_integer_to_float_audio`, configure `FullParams::new(SamplingStrategy::Greedy { best_of: 1 })` with `set_language(Some(&self.language))`, `set_print_special(false)`, `set_print_progress(false)`, `set_print_realtime(false)`, `set_print_timestamps(false)`. Run `state.full(params, &audio_f32)`. Iterate segments and concatenate text. Trim whitespace. If transcription returns an error, log to stderr and return `Ok(String::new())` (skip segment, don't crash).
    - `pub fn scan_models(dir: &Path) -> Result<Vec<(String, PathBuf)>>`: if the directory does not exist, create it with `std::fs::create_dir_all` and return `Ok(vec![])`. Otherwise scan for `ggml-*.bin` files, return vec of `(model_display_name, path)`. Extract display name from filename (e.g. `ggml-base.bin` → `"base"`).
    - Default model directory: `~/.local/share/space-stt/models/`.
  - Notes: `Transcriber` is `!Send` (WhisperContext). Must live on a dedicated transcription thread. Audio segments arrive via channel. Language is passed at construction time to allow future configurability without API changes.

- [ ] **Task 6: Hotkey module**
  - File: `src/hotkey.rs`
  - Action: Create module with:
    - `pub fn list_keyboards() -> Result<Vec<(PathBuf, String)>>`: enumerate `/dev/input/event*` via `evdev::enumerate()`. This returns `Vec<(PathBuf, Device)>` — path first, device second. Filter devices that have `EventType::KEY` capability. Map to `(device_path, device_name)` preserving the path-first order from the API. Extract name via `device.name().unwrap_or("Unknown").to_string()`.
    - `pub fn listen_hotkey(device_path: PathBuf, key: evdev::Key, is_listening: Arc<AtomicBool>) -> Result<()>`: open the evdev device, enter blocking loop on `device.fetch_events()`. This implements **hold-to-talk** semantics (NOT a toggle):
      - Key pressed (value=1): set `is_listening.store(true, Ordering::SeqCst)`.
      - Key released (value=0): set `is_listening.store(false, Ordering::SeqCst)`.
      - Key repeat (value=2): ignore.
    - If the evdev device returns an error (e.g. device unplugged), log to stderr: `"Hotkey device lost: {error}. Push-to-talk disabled."` and set `is_listening` to `false`. Exit the loop (don't crash the whole app).
    - The function is designed to run on a dedicated `std::thread::spawn` thread.
  - Notes: The `AtomicBool` is named `is_listening` to clearly reflect the hold-to-talk semantics. The pipeline checks `is_listening.load(Ordering::SeqCst)` to know if push-to-talk is active. Default hotkey suggestion: `Key::KEY_SCROLLLOCK` (rarely used, no side effects). Use `Ordering::SeqCst` (not `Relaxed`) to ensure visibility across threads.

- [ ] **Task 7: TUI startup module**
  - File: `src/tui.rs`
  - Action: Create module with a `pub fn run_setup() -> Result<SetupConfig>` function that displays three sequential selection screens using ratatui:
    - **Screen 1 — Audio Device**: call `audio::list_input_devices()`, present as a `List` widget. User navigates with arrow keys, selects with Enter.
    - **Screen 2 — Whisper Model**: call `transcribe::scan_models()`, present available models with their file sizes. If no models found, display error message with download instructions and the path to the models directory, then exit.
    - **Screen 3 — Push-to-Talk Key**: present a predefined list of common key choices: `ScrollLock`, `Pause`, `F9`, `F10`, `F11`, `F12`. User selects with arrow keys + Enter (same interaction pattern as screens 1 and 2). No raw evdev capture in TUI — keep it simple and consistent.
    - Return:
      ```rust
      pub struct SetupConfig {
          pub device: cpal::Device,
          pub model_path: PathBuf,
          pub hotkey: evdev::Key,
          pub keyboard_path: PathBuf,
          pub language: String,      // "fr" for MVP
          pub xkb_layout: String,    // "fr" for MVP
      }
      ```
    - For MVP, `language` and `xkb_layout` are set to `"fr"` inside `run_setup()` without a TUI screen. They are fields on `SetupConfig` so a future TUI screen can expose them without API changes.
    - Use `crossterm` as the ratatui backend. Enter raw mode at start, restore terminal on exit (including on panic — use a panic hook).
  - Notes: Terminal must be fully restored before the pipeline starts (pipeline prints status to stderr). The TUI phase is blocking and runs on the main thread. The pipeline also runs on the main thread after TUI completes. `SetupConfig` is consumed on the same thread — `cpal::Device` never crosses a thread boundary. All three screens use the same interaction pattern (arrow keys + Enter) for consistency.

- [ ] **Task 8: Pipeline orchestration and main entry point**
  - File: `src/main.rs`
  - Action: Implement the full pipeline:
    1. Run `tui::run_setup()` to get `SetupConfig`. Extract `device`, `model_path`, `hotkey`, `keyboard_path`, `language`, `xkb_layout`.
    2. Print status to stderr: `"Loading model {model_name}..."`. Create the transcription thread (with warm-up):
       - Channel: `crossbeam_channel::bounded::<Vec<i16>>(4)` (VAD→transcription, bounded to 4 segments to cap memory).
       - Channel: `crossbeam_channel::bounded::<String>(4)` (transcription→main, bounded to 4 results).
       - Spawn thread: create `Transcriber::new(&model_path, &language)`. **Warm-up**: immediately transcribe 1 second of silence (16000 zeros as i16) and discard the result — this forces whisper.cpp to fully initialize its compute graph and GPU allocations, so the first real transcription is not abnormally slow. Then loop on receiver, transcribe each segment, send result. When the receiver is disconnected (sender dropped), exit the thread.
    3. Print status to stderr: `"Starting audio capture on {device_name}..."`.
    4. Create audio→main channel: `crossbeam_channel::bounded::<Vec<i16>>(64)`. Note: 64 is a conservative buffer — at typical cpal callback sizes of ~1024 samples, this represents ~4 seconds of audio at 48kHz. If the main thread falls behind, samples are dropped in the cpal callback (non-blocking `try_send`), which is acceptable.
    5. Start `audio::start_capture()` with the channel sender. Capture the returned `CaptureConfig` to get the actual sample rate.
    6. Create the resampler: `audio::create_resampler(capture_config.sample_rate, 16000, capture_config.channels)`.
    7. Create `Arc<AtomicBool>` named `is_listening` (default: `false`).
    8. Spawn hotkey thread: `hotkey::listen_hotkey(keyboard_path, hotkey, is_listening.clone())`.
    9. Create `Injector::new(&xkb_layout)` for dotool.
    10. Set up Ctrl+C handler via `ctrlc::set_handler` — sets an `Arc<AtomicBool>` named `shutdown` to `true`.
    11. Print status to stderr: `"Ready. Hold [hotkey_name] to speak."`.
    12. **Main processing loop** (on main thread, because VAD is `!Send`):
        - Create `VoiceDetector`.
        - Track `was_listening: bool` to detect PTT state transitions.
        - Loop: receive audio chunks from cpal channel with `recv_timeout(Duration::from_millis(100))` to remain responsive to shutdown.
        - If `shutdown` is true, break.
        - Read `is_listening` state.
        - **PTT release detection**: if `was_listening == true` and `is_listening == false`, call `voice_detector.reset()` to discard any incomplete speech segment.
        - Update `was_listening = is_listening`.
        - If `is_listening` is false, discard samples and continue.
        - Resample the audio chunk to 16kHz mono via the resampler closure.
        - Feed resampled samples to `voice_detector.process_samples()`.
        - For each completed speech segment, send to transcription thread via channel. If channel is full (transcription backlog), log warning to stderr: `"Transcription busy, segment dropped."` and drop the segment.
        - Receive transcribed text from transcription thread (non-blocking `try_recv`).
        - If text received and non-empty, call `injector.type_text(&text)`. If injection fails, log error to stderr and continue (don't crash).
        - Print status line to stderr: `[LISTENING]` / `[PAUSED]` / `[TRANSCRIBING...]`.
    13. **Graceful shutdown** (after loop breaks):
        - Drop the audio stream (stops capture).
        - Drop channel senders (signals transcription thread to exit).
        - Join transcription thread with 10-second timeout. If it doesn't join within 10 seconds, log warning `"Transcription thread did not stop within 10s, exiting anyway."` and continue shutdown.
        - Drop `Injector` (kills dotool, waits for process).
        - Print `"Shutdown complete."` to stderr.
  - Notes: Status and error output goes to **stderr** (not stdout) to avoid interfering with the focused terminal where text is injected. All channels are bounded to prevent unbounded memory growth. The main loop uses `recv_timeout` instead of blocking `recv` to remain responsive to the shutdown signal. The PTT-release detection ensures incomplete segments are discarded cleanly.

### Acceptance Criteria

- [ ] **AC 1**: Given dotool is installed and user is in `input` group with uinput access, when `Injector::new("fr")` is called, then the preflight checks pass, dotool spawns, and the Injector is ready.
- [ ] **AC 2**: Given dotool is NOT installed, when `Injector::new("fr")` is called, then a clear error message mentioning the install URL is returned.
- [ ] **AC 3**: Given `/dev/uinput` is not accessible, when `Injector::new("fr")` is called, then a clear error message about the `input` group is returned.
- [ ] **AC 4**: Given a working Injector with `xkb_layout=fr`, when `type_text("hello world")` is called, then "hello world" appears as typed text in the focused window.
- [ ] **AC 5**: Given a working Injector with `xkb_layout=fr`, when `type_text("café résumé")` is called, then "café résumé" appears correctly with accented characters.
- [ ] **AC 6**: Given text containing newlines and control chars (`"line1\nline2\0foo\x01bar"`), when `sanitize()` is called, then `"line1 line2 foobar"` is returned (newlines→spaces, null/control chars removed).
- [ ] **AC 7**: Given a working Injector, when dotool crashes and `type_text()` is called, then the Injector respawns dotool and retries. If respawn succeeds, text is injected. If it fails, an error is logged.
- [ ] **AC 8**: Given a microphone is connected, when `list_input_devices()` is called, then at least one input device is returned with a human-readable name.
- [ ] **AC 9**: Given a valid input device (e.g. 48kHz native rate), when `start_capture()` is called, then i16 audio samples are received through the channel and `CaptureConfig` reflects the actual sample rate.
- [ ] **AC 10**: Given a resampler created with source=48000 target=16000, when fed 4800 samples (100ms at 48kHz), then approximately 1600 samples (100ms at 16kHz) are returned.
- [ ] **AC 11**: Given a resampler created with source=16000 target=16000, when fed samples, then the same samples are returned unchanged (no-op path).
- [ ] **AC 12**: Given a stream of 16kHz mono audio samples, when a voice segment is followed by 500ms+ of silence, then `VoiceDetector::process_samples()` returns exactly one completed segment containing the voiced audio including pre-roll frames (50ms before voice onset).
- [ ] **AC 13**: Given a stream of silence-only samples, when processed by `VoiceDetector`, then no segments are emitted.
- [ ] **AC 14**: Given `VoiceDetector` has accumulated partial speech, when `reset()` is called, then all internal buffers are cleared and no segment is emitted.
- [ ] **AC 15**: Given a valid Whisper model file, when `Transcriber::new(path, "fr")` is called, then the model loads without error.
- [ ] **AC 16**: Given a pre-recorded 16kHz mono i16 raw PCM file containing spoken French (provided out-of-band as `tests/fixtures/bonjour.raw`, format: little-endian signed 16-bit, 16000 Hz, mono, ~2-3 seconds of "bonjour le monde"), when `Transcriber::transcribe()` is called with its contents, then the returned string contains "bonjour" (case-insensitive). This test requires `ggml-tiny.bin` and is marked `#[ignore]`.
- [ ] **AC 17**: Given `~/.local/share/space-stt/models/` contains `ggml-base.bin`, when `scan_models()` is called, then it returns `[("base", path)]`.
- [ ] **AC 18**: Given `~/.local/share/space-stt/models/` does not exist, when `scan_models()` is called, then the directory is created and `Ok(vec![])` is returned.
- [ ] **AC 19**: Given an evdev keyboard device, when the configured hotkey is held down, then `is_listening` AtomicBool is `true`; when released, it is `false` (hold-to-talk, NOT toggle).
- [ ] **AC 20**: Given the evdev hotkey device is unplugged during runtime, when the hotkey thread detects the error, then `is_listening` is set to `false`, an error is logged to stderr, and the application continues running.
- [ ] **AC 21**: Given the application is launched, when the TUI appears, then three sequential screens (device, model, hotkey) allow selection via arrow keys + Enter.
- [ ] **AC 22**: Given the TUI panics or exits early, when control returns, then the terminal is fully restored (no raw mode artifacts).
- [ ] **AC 23**: Given the full pipeline is running and push-to-talk is held, when the user speaks a French sentence into the microphone, then the sentence appears as typed text in the focused window. (Manual test — latency depends on model size.)
- [ ] **AC 24**: Given the full pipeline is running and push-to-talk is NOT held, when the user speaks, then no text is injected.
- [ ] **AC 25**: Given the full pipeline is running and push-to-talk is released mid-speech, when the VAD has accumulated partial audio, then the partial segment is discarded (not transcribed).
- [ ] **AC 26**: Given the application is running, when the user presses Ctrl+C, then the application shuts down gracefully within 15 seconds (audio stopped, dotool killed, threads joined or timed out).

## Additional Context

### Dependencies

#### Cargo Dependencies (Cargo.toml)

```toml
[features]
default = []
cuda = ["whisper-rs/cuda"]

[dependencies]
whisper-rs = "0.15"
cpal = "0.17"
webrtc-vad = "0.4"
rubato = "0.16"
evdev = "0.12"
ratatui = "0.29"
crossterm = "0.28"
crossbeam-channel = "0.5"
ctrlc = { version = "3", features = ["termination"] }
anyhow = "1"
```

Note: `whisper-rs` pulls in build-time dependencies (`cc`, `cmake-rs`) via its own `build.rs`. No explicit `[build-dependencies]` needed in our Cargo.toml.

#### System Dependencies

- CMake (whisper.cpp build)
- C/C++ compiler (whisper.cpp build)
- ALSA dev headers (cpal ALSA backend)
- CUDA Toolkit (optional, for GPU acceleration via `--features cuda`)
- dotool (text injection, build from source)
- Go compiler + libxkbcommon-dev (dotool build)

### Testing Strategy

#### Unit Tests

- `inject.rs`: test `sanitize()` function: verify newlines→spaces, null bytes removed, control characters stripped, empty string after sanitize returns no-op. Test `Injector::new()` preflight checks (requires system access, skip in CI without uinput).
- `audio.rs`: test `list_input_devices()` returns at least one device (skip in CI if no audio hardware). Test `create_resampler` with known input/output sample counts (48kHz→16kHz: 4800 samples in → ~1600 out). Test no-op resampler (16kHz→16kHz).
- `vad.rs`: test with synthetic audio — all-zeros (silence) produces no segments. Test with synthetic loud samples (non-zero) followed by silence produces one segment. Verify emitted segment includes pre-roll frames. Test multiple speech bursts produce multiple segments. Test `reset()` clears state and discards accumulated audio.
- `transcribe.rs`: test `scan_models()` with a temp directory containing fake `.bin` files. Test `scan_models()` with non-existent directory (should create it, return empty vec). Test `Transcriber::new()` with a real model file (integration, requires model download, marked `#[ignore]`).
- `hotkey.rs`: test `list_keyboards()` returns devices with `(PathBuf, String)` tuples — path first (skip in CI if no `/dev/input` access).

#### Integration Tests

- End-to-end test requires real hardware (mic, dotool, evdev). Mark as `#[ignore]` for CI, run manually.
- Smoke test: launch app, verify TUI renders, select defaults, verify pipeline starts and status is printed to stderr.
- Transcription test: use `tests/fixtures/bonjour.raw` (pre-recorded, must be provided out-of-band). Format: little-endian signed 16-bit PCM, 16000 Hz, mono, ~2-3 seconds of spoken "bonjour le monde". Requires `ggml-tiny.bin`. Marked `#[ignore]`.

#### Manual Testing Checklist

- [ ] Launch app, verify TUI device selection works
- [ ] Verify TUI model selection lists downloaded models
- [ ] Verify TUI shows download instructions if no models present
- [ ] Select hotkey from list, verify push-to-talk hold=listen / release=stop
- [ ] Speak a French sentence while holding push-to-talk, verify text appears in focused terminal
- [ ] Verify accented characters (`é`, `à`, `ç`) are injected correctly
- [ ] Release push-to-talk mid-sentence, verify partial segment is discarded (no garbled text injected)
- [ ] Verify no text injection when push-to-talk is released
- [ ] Verify Ctrl+C shuts down cleanly
- [ ] Kill dotool manually during runtime, verify it respawns and continues working
- [ ] Unplug USB keyboard used for hotkey, verify app continues (no crash, PTT disabled message on stderr)
- [ ] Test with a 48kHz device — verify resampling works transparently
- [ ] Test on laptop (CPU-only, tiny model) — verify it works, note latency

### Notes

#### High-Risk Items

- **CUDA build on Fedora**: whisper-rs CUDA builds have had intermittent failures (whisper-rs issue #173). Test early. Fallback: CPU-only build (default feature set).
- **webrtc-vad crate age**: unmaintained since 2019. If it fails to compile on newer Rust toolchains, replace with Silero VAD via `ort` crate.
- **rubato resampling quality**: verify that `SincFixedIn` at default quality settings doesn't introduce audible artifacts that degrade Whisper transcription accuracy. If issues arise, try `FftFixedIn` as alternative.

#### Known Limitations

- No hot-swap of audio device during runtime — must restart app.
- No configuration file — all settings selected via TUI at startup each time.
- No wake word detection — push-to-talk only.
- Whisper transcription latency depends on segment length and model size — expect 1-5 seconds.
- No punctuation control — Whisper adds punctuation based on its own model.
- Language and XKB layout hardcoded to French (`"fr"`) at TUI level — fields exist on `SetupConfig` for future TUI exposure without API changes.
- PTT release mid-speech discards the segment — user must finish sentence before releasing.
- Whisper can hallucinate text on background noise (fan, music, ambient sounds) — push-to-talk mitigates this risk by preventing unintended injection. Never leave PTT active without speaking.

#### Future Considerations (Out of Scope)

- TTS feedback (Claude reads responses aloud) via Kokoro or Piper
- Configuration file to remember last-used settings
- Always-listening mode with wake word
- Streaming transcription for lower latency (whisper-stream-rs)
- Visual feedback overlay (small floating indicator showing listening state)
- Custom vocabulary / hotwords for technical terms
- Language / XKB layout selection in TUI
- Audio device format filtering in TUI (show only compatible devices)
