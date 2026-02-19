use anyhow::{Result, bail};
use cpal::traits::{DeviceTrait, HostTrait};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use evdev::KeyCode as EvdevKeyCode;
use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use std::path::PathBuf;
use std::time::Duration;

use crate::inject;
use crate::remote;
use crate::transcribe;

pub enum Backend {
    Local {
        model_path: PathBuf,
    },
    Remote {
        ssh_target: String,
        remote_model_path: String,
    },
}

pub struct SetupConfig {
    pub backend: Backend,
    pub device: cpal::Device,
    pub device_name: String,
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

    let mut terminal = ratatui::init();

    // Screen 1: Backend selection
    let backend_choices = vec![
        "Local (this machine)".to_string(),
        "Remote (SSH)".to_string(),
    ];
    let backend_idx = match select_screen(&mut terminal, "Select Backend", &backend_choices) {
        Ok(idx) => idx,
        Err(e) => {
            ratatui::restore();
            return Err(e);
        }
    };

    let backend = if backend_idx == 0 {
        // Local backend: scan local models
        let models_dir = transcribe::default_models_dir();
        let models = transcribe::scan_models(&models_dir)?;
        if models.is_empty() {
            ratatui::restore();
            bail!(
                "No Whisper models found in {}.\nDownload one:\n  wget -P {} https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
                models_dir.display(),
                models_dir.display()
            );
        }

        let model_labels: Vec<String> = models
            .iter()
            .map(|(name, path)| {
                let size = std::fs::metadata(path)
                    .map(|m| format_size(m.len()))
                    .unwrap_or_default();
                format!("{name} ({size})")
            })
            .collect();
        let model_idx =
            match select_screen(&mut terminal, "Select Whisper Model", &model_labels) {
                Ok(idx) => idx,
                Err(e) => {
                    ratatui::restore();
                    return Err(e);
                }
            };

        Backend::Local {
            model_path: models[model_idx].1.clone(),
        }
    } else {
        // Remote backend: get SSH target, then discover remote models
        let ssh_target = match text_input_screen(
            &mut terminal,
            "SSH Target",
            "user@host",
        ) {
            Ok(t) => t,
            Err(e) => {
                ratatui::restore();
                return Err(e);
            }
        };

        // Temporarily restore terminal to show SSH connection output
        ratatui::restore();
        let models = remote::list_remote_models(&ssh_target)?;
        if models.is_empty() {
            bail!("No Whisper models found on remote machine {ssh_target}.");
        }
        terminal = ratatui::init();

        let model_labels: Vec<String> = models
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let model_idx = match select_screen(
            &mut terminal,
            "Select Remote Model",
            &model_labels,
        ) {
            Ok(idx) => idx,
            Err(e) => {
                ratatui::restore();
                return Err(e);
            }
        };

        Backend::Remote {
            ssh_target,
            remote_model_path: models[model_idx].1.clone(),
        }
    };

    // Language selection
    let language_choices = vec![
        "English".to_string(),
        "Français".to_string(),
        "Deutsch".to_string(),
        "Español".to_string(),
        "Italiano".to_string(),
        "Português".to_string(),
        "日本語".to_string(),
        "中文".to_string(),
    ];
    let language_idx =
        match select_screen(&mut terminal, "Select Language", &language_choices) {
            Ok(idx) => idx,
            Err(e) => {
                ratatui::restore();
                return Err(e);
            }
        };
    let language = match language_idx {
        0 => "en",
        1 => "fr",
        2 => "de",
        3 => "es",
        4 => "it",
        5 => "pt",
        6 => "ja",
        7 => "zh",
        _ => "en",
    };

    // Push-to-Talk Key selection
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

    Ok(SetupConfig {
        backend,
        device,
        device_name,
        hotkey,
        language: language.to_string(),
        xkb_layout: inject::detect_xkb_layout(),
    })
}

fn text_input_screen(
    terminal: &mut ratatui::DefaultTerminal,
    title: &str,
    placeholder: &str,
) -> Result<String> {
    let mut input = String::new();

    loop {
        let display_text = if input.is_empty() {
            placeholder.to_string()
        } else {
            input.clone()
        };
        let is_empty = input.is_empty();
        let title = format!(" {title} (Enter=confirm, Esc=cancel) ");

        terminal.draw(|frame: &mut Frame| {
            let area = frame.area();
            let style = if is_empty {
                Style::default().add_modifier(Modifier::DIM)
            } else {
                Style::default()
            };
            let paragraph = Paragraph::new(format!("{display_text}_"))
                .style(style)
                .block(Block::default().borders(Borders::ALL).title(title));
            frame.render_widget(paragraph, area);
        })?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char(c) => input.push(c),
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Enter => {
                    let trimmed = input.trim().to_string();
                    if trimmed.is_empty() {
                        continue;
                    }
                    return Ok(trimmed);
                }
                KeyCode::Esc => {
                    bail!("Setup cancelled by user.");
                }
                _ => {}
            }
        }
    }
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
