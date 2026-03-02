use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::thread;
use std::time::Duration;

use crate::session;

/// Check if a prompt is ready (appears at the end of session output)
pub fn check_prompt_ready(session: &str, prompt: &str) -> Result<()> {
    let stdout_path = session::stdout_path(session)?;

    if !stdout_path.exists() {
        anyhow::bail!("no stdout at {} (is the session running?)", stdout_path.display());
    }

    let mut file = File::open(&stdout_path)
        .with_context(|| format!("failed to open {}", stdout_path.display()))?;

    // Read last 200 bytes
    let file_size = file.seek(SeekFrom::End(0))?;
    let read_size = std::cmp::min(200, file_size);
    let start_pos = file_size - read_size;

    file.seek(SeekFrom::Start(start_pos))?;

    let mut buffer = vec![0u8; read_size as usize];
    file.read_exact(&mut buffer)
        .with_context(|| "failed to read from stdout")?;

    // Process the content
    let last_content = process_terminal_output(&buffer)?;

    // Check if it ends with the prompt
    if !last_content.ends_with(prompt) {
        eprintln!("Session not ready. Expected prompt '{}' at end, but found:", prompt);
        eprintln!();

        // Show the content with visible control characters
        let mut stderr = std::io::stderr();
        for &byte in last_content.as_bytes() {
            if byte < 32 || byte == 127 {
                // Show control characters in caret notation
                match byte {
                    0..=26 => write!(stderr, "^{}", (byte + 64) as char)?,
                    27 => write!(stderr, "^[")?,
                    28 => write!(stderr, "^\\")?,
                    29 => write!(stderr, "^]")?,
                    30 => write!(stderr, "^^")?,
                    31 => write!(stderr, "^_")?,
                    127 => write!(stderr, "^?")?,
                    _ => write!(stderr, "{}", byte as char)?,
                }
            } else {
                write!(stderr, "{}", byte as char)?;
            }
        }
        writeln!(stderr)?;
        eprintln!();

        anyhow::bail!("Session may contain unprocessed input");
    }

    Ok(())
}

/// Wait for a new prompt to appear after the current position
pub fn wait_for_prompt(session: &str, prompt: &str) -> Result<()> {
    let stdout_path = session::stdout_path(session)?;

    if !stdout_path.exists() {
        anyhow::bail!("no stdout at {} (is the session running?)", stdout_path.display());
    }

    // Get initial file size
    let mut file = File::open(&stdout_path)
        .with_context(|| format!("failed to open {}", stdout_path.display()))?;
    let initial_size = file.seek(SeekFrom::End(0))?;

    // Poll for new content with prompt
    let max_attempts = 100; // 10 seconds total
    let poll_interval = Duration::from_millis(100);

    for _ in 0..max_attempts {
        thread::sleep(poll_interval);

        let mut file = File::open(&stdout_path)
            .with_context(|| format!("failed to open {}", stdout_path.display()))?;
        let current_size = file.seek(SeekFrom::End(0))?;

        // Check if file has grown
        if current_size > initial_size {
            // Read the new content plus some context
            let read_size = std::cmp::min(200, current_size);
            let start_pos = current_size - read_size;

            file.seek(SeekFrom::Start(start_pos))?;
            let mut buffer = vec![0u8; read_size as usize];
            file.read_exact(&mut buffer)
                .with_context(|| "failed to read from stdout")?;

            // Process and check if it ends with prompt
            let content = process_terminal_output(&buffer)?;
            if content.ends_with(prompt) {
                return Ok(());
            }
        }
    }

    anyhow::bail!("timeout waiting for prompt '{}'", prompt)
}

/// Strip all ANSI escape sequences from raw bytes using the vte parser.
pub fn strip_ansi(data: &[u8]) -> String {
    let stripped = strip_ansi_escapes::strip(data);
    String::from_utf8_lossy(&stripped).to_string()
}

/// Process terminal output to clean up escape sequences and control characters.
/// Uses the vte terminal parser (via strip-ansi-escapes) for robust handling
/// of all escape sequence types (CSI, OSC, DEC private modes, etc.).
pub fn process_terminal_output(data: &[u8]) -> Result<String> {
    // 1. Strip all ANSI/VT escape sequences
    let mut content = strip_ansi(data);

    // 2. Process carriage returns: split on \r and take the last segment
    if let Some(last_segment) = content.split('\r').last() {
        content = last_segment.to_string();
    }

    // 3. Remove control characters: bell (^G/\x07), backspace (^H/\x08), delete (\x7F)
    content = content.chars()
        .filter(|&c| c != '\x07' && c != '\x08' && c != '\x7F')
        .collect();

    // 4. Remove trailing whitespace
    content = content.trim_end().to_string();

    Ok(content)
}
