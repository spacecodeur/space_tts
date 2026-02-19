use anyhow::{Result, bail};
use cpal::traits::{DeviceTrait, HostTrait};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use evdev::KeyCode as EvdevKeyCode;
use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use std::path::PathBuf;
use std::time::Duration;

use crate::inject;
use crate::transcribe;

pub struct SetupConfig {
    pub device: cpal::Device,
    pub device_name: String,
    pub model_path: PathBuf,
    pub hotkey: EvdevKeyCode,
    pub language: String,
    pub xkb_layout: String,
}

pub fn run_setup() -> Result<SetupConfig> {
    // Auto-detect default audio input device (routes through PipeWire on modern Linux)
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("No default audio input device found."))?;
    let device_name = device
        .description()
        .map(|d: cpal::DeviceDescription| d.name().to_string())
        .unwrap_or_else(|_| "Default".into());

    let models_dir = transcribe::default_models_dir();
    let models = transcribe::scan_models(&models_dir)?;
    if models.is_empty() {
        bail!(
            "No Whisper models found in {}.\nDownload one:\n  wget -P {} https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
            models_dir.display(),
            models_dir.display()
        );
    }

    let mut terminal = ratatui::init();

    // Screen 1: Whisper Model
    let model_labels: Vec<String> = models
        .iter()
        .map(|(name, path)| {
            let size = std::fs::metadata(path)
                .map(|m| format_size(m.len()))
                .unwrap_or_default();
            format!("{name} ({size})")
        })
        .collect();
    let model_idx = match select_screen(&mut terminal, "Select Whisper Model", &model_labels) {
        Ok(idx) => idx,
        Err(e) => {
            ratatui::restore();
            return Err(e);
        }
    };

    // Screen 2: Push-to-Talk Key
    let hotkey_choices = vec![
        "F2".to_string(),
        "F3".to_string(),
        "F4".to_string(),
        "F9".to_string(),
        "F10".to_string(),
        "F11".to_string(),
        "F12".to_string(),
        "ScrollLock".to_string(),
        "Pause".to_string(),
    ];
    let hotkey_idx = match select_screen(&mut terminal, "Select Push-to-Talk Key", &hotkey_choices)
    {
        Ok(idx) => idx,
        Err(e) => {
            ratatui::restore();
            return Err(e);
        }
    };

    ratatui::restore();

    let hotkey = match hotkey_idx {
        0 => EvdevKeyCode::KEY_F2,
        1 => EvdevKeyCode::KEY_F3,
        2 => EvdevKeyCode::KEY_F4,
        3 => EvdevKeyCode::KEY_F9,
        4 => EvdevKeyCode::KEY_F10,
        5 => EvdevKeyCode::KEY_F11,
        6 => EvdevKeyCode::KEY_F12,
        7 => EvdevKeyCode::KEY_SCROLLLOCK,
        8 => EvdevKeyCode::KEY_PAUSE,
        _ => EvdevKeyCode::KEY_F2,
    };

    let model_path = models[model_idx].1.clone();

    Ok(SetupConfig {
        device,
        device_name,
        model_path,
        hotkey,
        language: "fr".to_string(),
        xkb_layout: inject::detect_xkb_layout(),
    })
}

fn select_screen(
    terminal: &mut ratatui::DefaultTerminal,
    title: &str,
    items: &[String],
) -> Result<usize> {
    let mut state = ListState::default();
    state.select(Some(0));

    loop {
        let title = title.to_string();
        let list_items: Vec<ListItem> = items.iter().map(|s| ListItem::new(s.as_str())).collect();

        terminal.draw(|frame: &mut Frame| {
            let area = frame.area();
            let list = List::new(list_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(" {title} (↑↓ Enter, q=quit) ")),
                )
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                .highlight_symbol("▸ ");
            frame.render_stateful_widget(list, area, &mut state);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Up => state.select_previous(),
                        KeyCode::Down => state.select_next(),
                        KeyCode::Enter => {
                            if let Some(idx) = state.selected() {
                                return Ok(idx);
                            }
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            bail!("Setup cancelled by user.");
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    }
}
