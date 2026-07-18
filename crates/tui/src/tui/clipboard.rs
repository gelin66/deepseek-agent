//! Clipboard text writer used by local TUI copy actions.

#[cfg(not(test))]
use anyhow::Context;
use anyhow::{Result, bail};
#[cfg(all(
    not(test),
    any(
        target_os = "macos",
        target_os = "windows",
        all(target_os = "linux", not(target_env = "ohos"))
    )
))]
use arboard::Clipboard;
use base64::Engine as _;
#[cfg(not(test))]
use std::io::{self, IsTerminal, Write};
#[cfg(any(
    all(not(test), target_os = "macos"),
    all(not(test), target_os = "windows"),
    all(not(test), target_os = "linux", not(target_env = "ohos"))
))]
use std::process::{Command, Stdio};

const OSC52_MAX_BYTES: usize = 100 * 1024;

/// Clipboard writer helper.
pub struct ClipboardHandler {
    #[cfg(all(
        not(test),
        any(
            target_os = "macos",
            target_os = "windows",
            all(target_os = "linux", not(target_env = "ohos"))
        )
    ))]
    clipboard: Option<Clipboard>,
    #[cfg(all(
        not(test),
        any(
            target_os = "macos",
            target_os = "windows",
            all(target_os = "linux", not(target_env = "ohos"))
        )
    ))]
    clipboard_init_attempted: bool,
    #[cfg(test)]
    written_text: Vec<String>,
}

impl ClipboardHandler {
    /// Create a new clipboard handler without connecting.
    ///
    /// The actual clipboard connection is deferred to first use
    /// (`ensure_clipboard`) so that startup on hosts without an X11/Wayland
    /// server (headless, WSL2) never blocks the TUI event loop.
    pub fn new() -> Self {
        Self {
            #[cfg(all(
                not(test),
                any(
                    target_os = "macos",
                    target_os = "windows",
                    all(target_os = "linux", not(target_env = "ohos"))
                )
            ))]
            clipboard: None,
            #[cfg(all(
                not(test),
                any(
                    target_os = "macos",
                    target_os = "windows",
                    all(target_os = "linux", not(target_env = "ohos"))
                )
            ))]
            clipboard_init_attempted: false,
            #[cfg(test)]
            written_text: Vec::new(),
        }
    }

    /// Try to connect to the system clipboard, bounded by a short timeout.
    ///
    /// On Linux, `arboard::Clipboard::new()` opens a blocking X11 connection.
    /// When no X server is running (headless, WSL2 without WSLg), the connect
    /// call can hang indefinitely. We spawn the connection attempt on a
    /// temporary thread and give it 500 ms; if it doesn't return in time the
    /// handler stays in fallback/no-op mode and `write_text` falls through to
    /// its OSC 52 and pbcopy/powershell fallbacks.
    #[cfg(all(
        not(test),
        any(
            target_os = "macos",
            target_os = "windows",
            all(target_os = "linux", not(target_env = "ohos"))
        )
    ))]
    fn ensure_clipboard(&mut self) {
        if self.clipboard_init_attempted {
            return;
        }
        self.clipboard_init_attempted = true;

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(Clipboard::new().ok());
        });
        self.clipboard = rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .ok()
            .flatten();
    }

    /// Write text to the clipboard (no-op if unavailable).
    pub fn write_text(&mut self, text: &str) -> Result<()> {
        #[cfg(test)]
        {
            self.written_text.push(text.to_string());
            Ok(())
        }

        #[cfg(not(test))]
        {
            #[cfg(all(target_os = "linux", not(target_env = "ohos")))]
            if write_text_with_wlcopy(text).is_ok() {
                return Ok(());
            }

            #[cfg(any(
                target_os = "macos",
                target_os = "windows",
                all(target_os = "linux", not(target_env = "ohos"))
            ))]
            {
                self.ensure_clipboard();
                if let Some(clipboard) = self.clipboard.as_mut()
                    && clipboard.set_text(text.to_string()).is_ok()
                {
                    return Ok(());
                }
            }

            #[cfg(target_os = "macos")]
            if write_text_with_pbcopy(text).is_ok() {
                return Ok(());
            }

            #[cfg(target_os = "windows")]
            if write_text_with_set_clipboard(text).is_ok() {
                return Ok(());
            }

            write_text_with_osc52(text)
                .map_err(|err| anyhow::anyhow!("Clipboard unavailable: {err}"))
        }
    }

    #[cfg(test)]
    pub fn last_written_text(&self) -> Option<&str> {
        self.written_text.last().map(String::as_str)
    }
}

#[cfg(all(target_os = "macos", not(test)))]
fn write_text_with_pbcopy(text: &str) -> Result<()> {
    write_text_with_stdin_command("pbcopy", &[], text, "pbcopy")
}

#[cfg(all(target_os = "windows", not(test)))]
fn write_text_with_set_clipboard(text: &str) -> Result<()> {
    write_text_with_stdin_command(
        "powershell.exe",
        &["-NoProfile", "-Command", "Set-Clipboard -Value $input"],
        text,
        "Set-Clipboard",
    )
}

#[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
fn write_text_with_stdin_command(
    program: &str,
    args: &[&str],
    text: &str,
    label: &str,
) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to run {label}: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to write to {label}: {e}"))?;
    }
    let _ = std::thread::Builder::new()
        .name("clipboard-wait".to_string())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

#[cfg(all(target_os = "linux", not(target_env = "ohos"), not(test)))]
fn write_text_with_wlcopy(text: &str) -> Result<()> {
    write_text_with_wlcopy_using_argv("wl-copy", text)
}

#[cfg(all(target_os = "linux", not(target_env = "ohos"), not(test)))]
fn write_text_with_wlcopy_using_argv(program: &str, text: &str) -> Result<()> {
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to run {program}: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to write to {program}: {e}"))?;
    }
    // stdin is dropped here, closing the pipe so wl-copy flushes.
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("Failed to wait on {program}: {e}"))?;
    if !status.success() {
        bail!("{program} exited with {status}");
    }
    Ok(())
}

#[cfg(not(test))]
fn write_text_with_osc52(text: &str) -> Result<()> {
    let mut stdout = io::stdout();
    if !stdout.is_terminal() {
        bail!("OSC 52 clipboard fallback requires a terminal");
    }

    let in_tmux = std::env::var_os("TMUX").is_some();
    let sequence = osc52_sequence(text, in_tmux)?;
    stdout
        .write_all(sequence.as_bytes())
        .context("write OSC 52 clipboard sequence")?;
    stdout.flush().context("flush OSC 52 clipboard sequence")
}

fn osc52_sequence(text: &str, in_tmux: bool) -> Result<String> {
    if text.len() > OSC52_MAX_BYTES {
        bail!("selection is too large for OSC 52 clipboard fallback");
    }

    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    if in_tmux {
        return Ok(format!("\x1bPtmux;\x1b{sequence}\x1b\\"));
    }
    Ok(sequence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_sequence_encodes_text_clipboard_write() {
        let sequence = osc52_sequence("hello", false).expect("sequence");
        assert_eq!(sequence, "\x1b]52;c;aGVsbG8=\x07");
    }

    #[test]
    fn osc52_sequence_wraps_for_tmux_passthrough() {
        let sequence = osc52_sequence("copy", true).expect("sequence");
        assert_eq!(sequence, "\x1bPtmux;\x1b\x1b]52;c;Y29weQ==\x07\x1b\\");
    }

    #[test]
    fn osc52_sequence_rejects_oversized_selection() {
        let text = "x".repeat(OSC52_MAX_BYTES + 1);
        let err = osc52_sequence(&text, false).expect_err("oversized should fail");
        assert!(
            err.to_string().contains("too large"),
            "unexpected error: {err}"
        );
    }
}
