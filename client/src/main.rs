mod audio;
mod hotkey;
mod inject;
mod remote;
mod tui;
mod vad;

use anyhow::Result;
use inject::TextInjector;
use remote::Transcriber;
use space_tts_common::{debug, info, warn};
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
                warn!("User is NOT in the 'input' group.");
                warn!("  This will block evdev hotkey and dotool uinput access.");
                warn!("  Fix: sudo usermod -aG input $USER && log out/in");
            }
        }
        Err(_) => {
            warn!("Could not check group membership (id command failed).");
        }
    }

    // Check /dev/uinput access
    match std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
    {
        Ok(_) => {}
        Err(e) => {
            warn!("Cannot open /dev/uinput: {e}");
            warn!("  dotool text injection will fail.");
            warn!("  Fix: sudo usermod -aG input $USER && log out/in");
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Parse --debug flag
    if args.iter().any(|a| a == "--debug") {
        space_tts_common::log::set_debug(true);
    }

    run_client()
}

fn run_client() -> Result<()> {
    info!("Space STT — Remote Speech-to-Text Terminal Injector");
    check_input_group();

    // 1. Run TUI setup
    let config = tui::run_setup()?;

    info!("  Backend:  Remote ({0})", config.ssh_target);
    info!("  Model:    {0}", config.remote_model_path);
    info!("  Device:   {}", config.device_name);
    info!("  Hotkey:   {:?}", config.hotkey);
    info!("  Language: {}", config.language);
    debug!("  XKB:      {}", config.xkb_layout);

    // 2. Set up transcription thread
    info!("Connecting to remote server...");

    let (seg_tx, seg_rx) = crossbeam_channel::bounded::<Vec<i16>>(4);
    let (text_tx, text_rx) = crossbeam_channel::bounded::<String>(4);

    let ssh_target = config.ssh_target.clone();
    let remote_model_path = config.remote_model_path.clone();
    let language = config.language.clone();

    let transcribe_handle = std::thread::Builder::new()
        .name("transcriber".into())
        .spawn(move || {
            let mut transcriber: Box<dyn Transcriber> =
                match remote::RemoteTranscriber::new(&ssh_target, &remote_model_path, &language) {
                    Ok(t) => Box::new(t),
                    Err(e) => {
                        info!("Failed to connect to remote: {e}");
                        return;
                    }
                };

            // Process segments from channel
            for segment in seg_rx {
                match transcriber.transcribe(&segment) {
                    Ok(text) if !text.is_empty() => {
                        if text_tx.send(text).is_err() {
                            break; // main thread dropped receiver
                        }
                    }
                    Ok(_) => {} // empty transcription, skip
                    Err(e) => debug!("Transcription error: {e}"),
                }
            }
        })?;

    // 3. Start audio capture
    let device_name = &config.device_name;
    debug!("Starting audio capture on {device_name}...");

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
    info!("Ready! Press {:?} to toggle listening.", config.hotkey);

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
            info!("[PAUSED]");
            debug!("  (processed {listening_chunks} audio chunks while listening)");
            listening_chunks = 0;
        }

        if !was_listening && listening {
            info!("[LISTENING]");
            listening_chunks = 0;
        }

        was_listening = listening;

        if !listening {
            // Log audio flow periodically to confirm capture works
            if chunk_count.is_multiple_of(500) {
                debug!(
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
            if listening_chunks.is_multiple_of(100) {
                debug!("  WARNING: resampler producing empty output");
            }
            continue;
        }

        // Log first chunk to confirm pipeline works
        if listening_chunks == 1 {
            debug!(
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
            debug!(
                "[TRANSCRIBING...] segment: {} samples ({:.0}ms)",
                segment.len(),
                duration_ms
            );
            if seg_tx.try_send(segment).is_err() {
                debug!("Transcription busy, segment dropped.");
            }
        }

        // Check for transcription results (non-blocking)
        while let Ok(text) = text_rx.try_recv() {
            info!("[RESULT] \"{}\"", text);
            if let Err(e) = injector.type_text(&text) {
                warn!("Injection error: {e}");
            }
        }
    }

    // 9. Graceful shutdown
    info!("Shutting down...");

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
        warn!("Transcription thread did not stop within 10s, exiting anyway.");
    }

    // Drop injector (kills dotool)
    drop(injector);

    info!("Shutdown complete.");
    Ok(())
}
