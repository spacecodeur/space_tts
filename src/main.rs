mod audio;
mod hotkey;
mod inject;
mod transcribe;
mod tui;
mod vad;

use anyhow::Result;
use inject::TextInjector;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn check_input_group() {
    // Check if current user is in the 'input' group
    let output = std::process::Command::new("id").arg("-Gn").output();
    match output {
        Ok(o) => {
            let groups = String::from_utf8_lossy(&o.stdout);
            if !groups.split_whitespace().any(|g| g == "input") {
                eprintln!("WARNING: User is NOT in the 'input' group.");
                eprintln!("  This will block evdev hotkey and dotool uinput access.");
                eprintln!("  Fix: sudo usermod -aG input $USER && log out/in");
                eprintln!();
            }
        }
        Err(_) => {
            eprintln!("WARNING: Could not check group membership (id command failed).");
        }
    }

    // Check /dev/uinput access
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/uinput")
    {
        Ok(_) => {}
        Err(e) => {
            eprintln!("WARNING: Cannot open /dev/uinput: {e}");
            eprintln!("  dotool text injection will fail.");
            eprintln!("  Fix: sudo usermod -aG input $USER && log out/in");
            eprintln!();
        }
    }
}

fn main() -> Result<()> {
    eprintln!("Space STT — Local Speech-to-Text Terminal Injector");
    check_input_group();

    // 1. Run TUI setup
    let config = tui::run_setup()?;

    let model_name = config
        .model_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let device_name = &config.device_name;

    eprintln!("--- Configuration ---");
    eprintln!("  Device:   {device_name}");
    eprintln!("  Model:    {model_name} ({})", config.model_path.display());
    eprintln!("  Hotkey:   {:?}", config.hotkey);
    eprintln!("  Language: {}", config.language);
    eprintln!("  XKB:      {}", config.xkb_layout);
    eprintln!("---------------------");

    // 2. Set up transcription thread with warm-up
    eprintln!("Loading model {model_name}...");

    let (seg_tx, seg_rx) = crossbeam_channel::bounded::<Vec<i16>>(4);
    let (text_tx, text_rx) = crossbeam_channel::bounded::<String>(4);

    let model_path = config.model_path.to_string_lossy().to_string();
    let language = config.language.clone();

    let transcribe_handle = std::thread::Builder::new()
        .name("transcriber".into())
        .spawn(move || {
            let mut transcriber = match transcribe::Transcriber::new(&model_path, &language) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Failed to load model: {e}");
                    return;
                }
            };

            // Warm-up: transcribe 1s of silence to init GPU graph
            eprintln!("Warming up whisper...");
            let silence = vec![0i16; 16000];
            let _ = transcriber.transcribe(&silence);
            eprintln!("Warm-up complete.");

            // Process segments from channel
            for segment in seg_rx {
                match transcriber.transcribe(&segment) {
                    Ok(text) if !text.is_empty() => {
                        if text_tx.send(text).is_err() {
                            break; // main thread dropped receiver
                        }
                    }
                    Ok(_) => {} // empty transcription, skip
                    Err(e) => eprintln!("Transcription error: {e}"),
                }
            }
        })?;

    // 3. Start audio capture
    eprintln!("Starting audio capture on {device_name}...");

    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<Vec<i16>>(64);
    let (_stream, capture_config) = audio::start_capture(&config.device, audio_tx)?;

    // 4. Create resampler
    let mut resample =
        audio::create_resampler(capture_config.sample_rate, 16000, capture_config.channels)?;

    // 5. Set up hotkey on all keyboards
    let is_listening = Arc::new(AtomicBool::new(false));
    hotkey::listen_all_keyboards(config.hotkey, is_listening.clone())?;

    // 6. Create injector
    let mut injector = inject::Injector::new(&config.xkb_layout)?;

    // 7. Set up Ctrl+C handler
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    ctrlc::set_handler(move || {
        shutdown_clone.store(true, Ordering::SeqCst);
    })?;

    // 8. Main processing loop
    eprintln!("Ready. Hold push-to-talk key to speak.");

    let mut voice_detector = vad::VoiceDetector::new()?;
    let mut was_listening = false;
    let mut chunk_count: u64 = 0;
    let mut listening_chunks: u64 = 0;

    loop {
        // Check shutdown
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // Receive audio chunk (with timeout to stay responsive)
        let chunk = match audio_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => c,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };

        chunk_count += 1;

        let listening = is_listening.load(Ordering::SeqCst);

        // PTT release detection: discard incomplete segment
        if was_listening && !listening {
            voice_detector.reset();
            eprintln!("[PAUSED] (processed {listening_chunks} audio chunks while listening)");
            listening_chunks = 0;
        }

        if !was_listening && listening {
            eprintln!("[LISTENING]");
            listening_chunks = 0;
        }

        was_listening = listening;

        if !listening {
            // Log audio flow periodically to confirm capture works
            if chunk_count % 500 == 0 {
                eprintln!(
                    "  (audio flowing: {chunk_count} chunks received, {} samples/chunk)",
                    chunk.len()
                );
            }
            continue; // discard samples when not listening
        }

        listening_chunks += 1;

        // Resample to 16kHz mono
        let resampled = resample(&chunk);
        if resampled.is_empty() {
            if listening_chunks % 100 == 0 {
                eprintln!("  WARNING: resampler producing empty output");
            }
            continue;
        }

        // Log first chunk to confirm pipeline works
        if listening_chunks == 1 {
            eprintln!(
                "  Audio chunk: {} samples -> resampled to {} samples",
                chunk.len(),
                resampled.len()
            );
        }

        // Feed to VAD
        let segments = voice_detector.process_samples(&resampled);

        // Send completed segments for transcription
        for segment in segments {
            let duration_ms = segment.len() as f64 / 16.0; // 16 samples per ms at 16kHz
            eprintln!(
                "[TRANSCRIBING...] segment: {} samples ({:.0}ms)",
                segment.len(),
                duration_ms
            );
            if seg_tx.try_send(segment).is_err() {
                eprintln!("Transcription busy, segment dropped.");
            }
        }

        // Check for transcription results (non-blocking)
        while let Ok(text) = text_rx.try_recv() {
            eprintln!("[RESULT] \"{}\"", text);
            if let Err(e) = injector.type_text(&text) {
                eprintln!("Injection error: {e}");
            }
        }
    }

    // 9. Graceful shutdown
    eprintln!("Shutting down...");

    // Drop stream (stops capture) and senders (signal threads to exit)
    drop(_stream);
    drop(seg_tx);

    // Wait for transcription thread to finish (segments channel is closed)
    // The thread will exit once seg_rx is drained/disconnected.
    // Use a 10-second timeout via a helper thread.
    let (done_tx, done_rx) = crossbeam_channel::bounded::<()>(1);
    std::thread::spawn(move || {
        let _ = transcribe_handle.join();
        let _ = done_tx.send(());
    });
    if done_rx.recv_timeout(Duration::from_secs(10)).is_err() {
        eprintln!("Transcription thread did not stop within 10s, exiting anyway.");
    }

    // Drop injector (kills dotool)
    drop(injector);

    eprintln!("Shutdown complete.");
    Ok(())
}
