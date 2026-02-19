use anyhow::Result;
use evdev::{Device, EventType, KeyCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use space_tts_common::{debug, warn};

/// List all keyboard-like evdev devices (filtering out non-keyboards).
fn find_keyboards() -> Vec<(std::path::PathBuf, String)> {
    evdev::enumerate()
        .filter(|(_, dev)| {
            if !dev.supported_events().contains(EventType::KEY) {
                return false;
            }
            let has_real_keys = dev
                .supported_keys()
                .map(|keys| keys.contains(KeyCode::KEY_A) || keys.contains(KeyCode::KEY_F1))
                .unwrap_or(false);
            if !has_real_keys {
                return false;
            }
            let name = dev.name().unwrap_or("").to_lowercase();
            !name.contains("power button")
                && !name.contains("sleep button")
                && !name.contains("led controller")
                && !name.contains("consumer control")
                && !name.contains("system control")
        })
        .map(|(path, dev)| {
            let name = dev.name().unwrap_or("Unknown").to_string();
            (path, name)
        })
        .collect()
}

/// Listen for the hotkey on ALL detected keyboards simultaneously.
/// Spawns one thread per keyboard device. Any of them pressing the key triggers PTT.
pub fn listen_all_keyboards(key: KeyCode, is_listening: Arc<AtomicBool>) -> Result<()> {
    let keyboards = find_keyboards();

    if keyboards.is_empty() {
        warn!("No keyboard devices found for hotkey. Is the user in the 'input' group?");
        return Ok(());
    }

    for (path, name) in keyboards {
        let is_listening = is_listening.clone();
        let path_display = path.display().to_string();

        std::thread::Builder::new()
            .name(format!(
                "hotkey-{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ))
            .spawn(move || {
                let mut device = match Device::open(&path) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!("Cannot open {path_display} ({name}): {e}");
                        return;
                    }
                };

                debug!("Hotkey listener on: {name} ({path_display})");

                loop {
                    match device.fetch_events() {
                        Ok(events) => {
                            for event in events {
                                if event.event_type() == EventType::KEY
                                    && event.code() == key.code()
                                    && event.value() == 1
                                {
                                    // Toggle on key press (not release, not repeat)
                                    let prev = is_listening.load(Ordering::SeqCst);
                                    is_listening.store(!prev, Ordering::SeqCst);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Hotkey device lost ({name}): {e}");
                            return;
                        }
                    }
                }
            })?;
    }

    Ok(())
}
