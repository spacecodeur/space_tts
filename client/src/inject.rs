use anyhow::{Context, Result, bail};
use std::io::Write;
use std::process::{Child, Command, Stdio};

use space_tts_common::warn;

pub trait TextInjector {
    fn type_text(&mut self, text: &str) -> Result<()>;
}

pub struct Injector {
    child: Child,
    xkb_layout: String,
}

impl Injector {
    pub fn new(xkb_layout: &str) -> Result<Self> {
        // Preflight: check /dev/uinput access
        let uinput = std::path::Path::new("/dev/uinput");
        if !uinput.exists() {
            bail!(
                "Cannot access /dev/uinput. Ensure your user is in the 'input' group and log out/in."
            );
        }
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(uinput)
        {
            Ok(_) => {}
            Err(_) => {
                bail!(
                    "Cannot access /dev/uinput. Ensure your user is in the 'input' group and log out/in."
                );
            }
        }

        // Preflight: check dotool in PATH
        let status = Command::new("which")
            .arg("dotool")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => {}
            _ => {
                bail!("dotool not found. Install it: https://git.sr.ht/~geb/dotool");
            }
        }

        let child = spawn_dotool(xkb_layout)?;
        Ok(Self {
            child,
            xkb_layout: xkb_layout.to_string(),
        })
    }

    fn respawn(&mut self) -> Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.child = spawn_dotool(&self.xkb_layout)?;
        Ok(())
    }
}

impl TextInjector for Injector {
    fn type_text(&mut self, text: &str) -> Result<()> {
        let sanitized = sanitize(text);
        if sanitized.is_empty() {
            return Ok(());
        }

        let cmd = format!("type {sanitized}\n");

        let write_result = (|| -> Result<()> {
            let stdin = self
                .child
                .stdin
                .as_mut()
                .context("dotool stdin not available")?;
            stdin.write_all(cmd.as_bytes())?;
            stdin.flush()?;
            Ok(())
        })();

        if write_result.is_err() {
            warn!("dotool pipe broken, respawning...");
            self.respawn()?;
            let stdin = self
                .child
                .stdin
                .as_mut()
                .context("dotool stdin not available after respawn")?;
            stdin.write_all(cmd.as_bytes())?;
            stdin.flush()?;
        }

        Ok(())
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_dotool(xkb_layout: &str) -> Result<Child> {
    let mut cmd = Command::new("dotool");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Split "us+altgr-intl" into DOTOOL_XKB_LAYOUT=us, DOTOOL_XKB_VARIANT=altgr-intl
    if let Some((layout, variant)) = xkb_layout.split_once('+') {
        cmd.env("DOTOOL_XKB_LAYOUT", layout);
        cmd.env("DOTOOL_XKB_VARIANT", variant);
    } else {
        cmd.env("DOTOOL_XKB_LAYOUT", xkb_layout);
    }

    cmd.spawn().context("Failed to spawn dotool")
}

/// Auto-detect the system XKB keyboard layout.
/// Returns a string like "us", "us+altgr-intl", "fr", etc.
pub fn detect_xkb_layout() -> String {
    if let Some(layout) = detect_from_gsettings() {
        return layout;
    }
    if let Some(layout) = detect_from_localectl() {
        return layout;
    }
    "us".to_string()
}

fn detect_from_gsettings() -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.input-sources", "sources"])
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_gsettings_layout(&stdout)
}

fn parse_gsettings_layout(output: &str) -> Option<String> {
    // Parse [('xkb', 'us+altgr-intl'), ('xkb', 'fr')] — extract first xkb layout
    let trimmed = output.trim();
    let start = trimmed.find("('xkb', '")? + "('xkb', '".len();
    let after = &trimmed[start..];
    let end = after.find('\'')?;
    let layout = &after[..end];
    if layout.is_empty() {
        None
    } else {
        Some(layout.to_string())
    }
}

fn detect_from_localectl() -> Option<String> {
    let output = Command::new("localectl")
        .arg("status")
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_localectl_layout(&stdout)
}

fn parse_localectl_layout(output: &str) -> Option<String> {
    let mut layout = None;
    let mut variant = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("X11 Layout:") {
            layout = Some(rest.trim().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("X11 Variant:") {
            let v = rest.trim().to_string();
            if !v.is_empty() {
                variant = Some(v);
            }
        }
    }

    match (layout, variant) {
        (Some(l), Some(v)) => Some(format!("{l}+{v}")),
        (Some(l), None) => Some(l),
        _ => None,
    }
}

pub fn sanitize(text: &str) -> String {
    let s: String = text
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' => Some(' '),
            '\0' => None,
            c if c as u32 <= 0x1F => None,
            c if (0x7F..=0x9F).contains(&(c as u32)) => None,
            c => Some(c),
        })
        .collect();
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_newlines_to_spaces() {
        assert_eq!(sanitize("line1\nline2"), "line1 line2");
        assert_eq!(sanitize("line1\r\nline2"), "line1  line2");
        assert_eq!(sanitize("line1\rline2"), "line1 line2");
    }

    #[test]
    fn sanitize_removes_null_and_control_chars() {
        assert_eq!(sanitize("line1\nline2\0foo\x01bar"), "line1 line2foobar");
    }

    #[test]
    fn sanitize_strips_whitespace() {
        assert_eq!(sanitize("  hello  "), "hello");
    }

    #[test]
    fn sanitize_empty_after_strip() {
        assert_eq!(sanitize("\n\0\x01"), "");
    }

    #[test]
    fn sanitize_preserves_unicode() {
        assert_eq!(sanitize("café résumé"), "café résumé");
    }

    #[test]
    fn sanitize_removes_delete_and_c1_controls() {
        // U+007F (DEL) and U+0080-U+009F (C1 controls)
        assert_eq!(sanitize("a\x7Fb"), "ab");
        assert_eq!(sanitize("a\u{0080}b"), "ab");
        assert_eq!(sanitize("a\u{009F}b"), "ab");
        // U+00A0 (non-breaking space) should be kept
        assert_eq!(sanitize("a\u{00A0}b"), "a\u{00A0}b");
    }

    #[test]
    fn parse_gsettings_single_layout() {
        let output = "[('xkb', 'us+altgr-intl')]\n";
        assert_eq!(
            parse_gsettings_layout(output),
            Some("us+altgr-intl".to_string())
        );
    }

    #[test]
    fn parse_gsettings_multiple_layouts() {
        let output = "[('xkb', 'fr'), ('xkb', 'us')]\n";
        assert_eq!(parse_gsettings_layout(output), Some("fr".to_string()));
    }

    #[test]
    fn parse_gsettings_no_xkb() {
        let output = "@as []\n";
        assert_eq!(parse_gsettings_layout(output), None);
    }

    #[test]
    fn parse_localectl_with_variant() {
        let output = "   System Locale: LANG=fr_FR.UTF-8\n       VC Keymap: us\n      X11 Layout: us\n     X11 Variant: altgr-intl\n";
        assert_eq!(
            parse_localectl_layout(output),
            Some("us+altgr-intl".to_string())
        );
    }

    #[test]
    fn parse_localectl_without_variant() {
        let output =
            "   System Locale: LANG=en_US.UTF-8\n       VC Keymap: us\n      X11 Layout: us\n";
        assert_eq!(parse_localectl_layout(output), Some("us".to_string()));
    }
}
