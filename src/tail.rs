use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::Command;

use crate::prompt;
use crate::session;

#[derive(Debug)]
enum TailMode {
    Plain,
    Since(String),
    Delim(String),
}

struct TailOptions {
    mode: TailMode,
    lines: Option<usize>,
    follow: bool,
}

/// Parse tail arguments
fn parse_tail_args(session: &str, args: &[String]) -> Result<TailOptions> {
    let mut mode = TailMode::Plain;
    let mut lines: Option<usize> = None;
    let mut follow = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-f" => {
                follow = true;
                i += 1;
            }
            "-n" => {
                if i + 1 >= args.len() {
                    anyhow::bail!("usage: via {} tail -n <N> [--since PROMPT|--delim PROMPT|-f]", session);
                }
                lines = Some(args[i + 1].parse()
                    .with_context(|| format!("invalid number: {}", args[i + 1]))?);
                i += 2;
            }
            "--since" => {
                if i + 1 >= args.len() {
                    anyhow::bail!("usage: via {} tail --since PROMPT", session);
                }
                if !matches!(mode, TailMode::Plain) {
                    anyhow::bail!("only one of --since/--delim allowed");
                }
                mode = TailMode::Since(args[i + 1].clone());
                i += 2;
            }
            "--delim" => {
                if i + 1 >= args.len() {
                    anyhow::bail!("usage: via {} tail --delim PROMPT", session);
                }
                if !matches!(mode, TailMode::Plain) {
                    anyhow::bail!("only one of --since/--delim allowed");
                }
                mode = TailMode::Delim(args[i + 1].clone());
                i += 2;
            }
            _ => break,
        }
    }

    // Validate combinations
    if follow && !matches!(mode, TailMode::Plain) {
        anyhow::bail!("-f cannot be combined with --since or --delim");
    }

    if matches!(mode, TailMode::Plain) && lines.is_none() && !follow {
        anyhow::bail!("usage: via <session> tail [-n N] (--since PROMPT | --delim PROMPT | -f)");
    }

    Ok(TailOptions { mode, lines, follow })
}

/// Tail a session's output
pub fn tail_session(session: &str, args: &[String]) -> Result<()> {
    let opts = parse_tail_args(session, args)?;
    let stdout_path = session::stdout_path(session)?;

    if !stdout_path.exists() {
        anyhow::bail!("no stdout at {}", stdout_path.display());
    }

    if opts.follow {
        // Use external tail -f command
        let mut cmd = Command::new("tail");
        cmd.arg("-f");
        if let Some(n) = opts.lines {
            cmd.arg("-n").arg(n.to_string());
        }
        cmd.arg(&stdout_path);

        let status = cmd.status()
            .with_context(|| "failed to execute tail -f")?;

        if !status.success() {
            anyhow::bail!("tail -f failed");
        }

        return Ok(());
    }

    match opts.mode {
        TailMode::Plain => {
            // Simple tail -n N
            let n = opts.lines.unwrap(); // Already validated
            let output = Command::new("tail")
                .arg("-n")
                .arg(n.to_string())
                .arg(&stdout_path)
                .output()
                .with_context(|| "failed to execute tail")?;

            std::io::Write::write_all(&mut std::io::stdout(), &output.stdout)?;
        }
        TailMode::Since(prompt) => {
            tail_since(session, &prompt, opts.lines.unwrap_or(100))?;
        }
        TailMode::Delim(prompt) => {
            tail_delim_internal(session, &prompt, opts.lines.unwrap_or(100))?;
        }
    }

    Ok(())
}

/// Tail since the last occurrence of a prompt (includes prompt)
fn tail_since(session: &str, prompt: &str, window: usize) -> Result<()> {
    let stdout_path = session::stdout_path(session)?;

    // Equivalent to: tail -n window | tac | grep -m 1 -B window -F prompt | tac
    // We'll implement this in Rust for better control

    let file = File::open(&stdout_path)
        .with_context(|| format!("failed to open {}", stdout_path.display()))?;

    let lines = read_last_n_lines(&file, window)?;

    // Find the last occurrence of prompt (strip ANSI before matching)
    if let Some(idx) = lines.iter().rposition(|line| prompt::strip_ansi(line.as_bytes()).contains(prompt)) {
        // Print from that line onwards
        for line in &lines[idx..] {
            println!("{}", line);
        }
    }

    Ok(())
}

/// Tail the last stanza delimited by prompt (from second-to-last prompt, excluding last prompt)
fn tail_delim_internal(session: &str, prompt: &str, window: usize) -> Result<()> {
    let stdout_path = session::stdout_path(session)?;

    let file = File::open(&stdout_path)
        .with_context(|| format!("failed to open {}", stdout_path.display()))?;

    let lines = read_last_n_lines(&file, window)?;

    // Find all occurrences of prompt (strip ANSI before matching)
    let indices: Vec<usize> = lines.iter()
        .enumerate()
        .filter(|(_, line)| prompt::strip_ansi(line.as_bytes()).contains(prompt))
        .map(|(i, _)| i)
        .collect();

    if indices.len() >= 2 {
        // Print from second-to-last prompt up to (but not including) the last prompt
        let start = indices[indices.len() - 2];
        let end = indices[indices.len() - 1];
        for line in &lines[start..end] {
            println!("{}", line);
        }
    } else if indices.len() == 1 {
        // Only one prompt found, print from there onwards
        let start = indices[0];
        for line in &lines[start..] {
            println!("{}", line);
        }
    }

    Ok(())
}

/// Public interface for tail_delim used by prompt shorthand
pub fn tail_delim(session: &str, prompt: &str) -> Result<()> {
    tail_delim_internal(session, prompt, 100)
}

/// Read the last N lines from a file efficiently
fn read_last_n_lines(file: &File, n: usize) -> Result<Vec<String>> {
    use std::io::{Seek, SeekFrom};

    let mut file = file.try_clone()?;
    let file_size = file.seek(SeekFrom::End(0))?;

    if file_size == 0 {
        return Ok(Vec::new());
    }

    // Estimate: assume average line is 80 chars
    let estimated_bytes = n * 80;
    let start_pos = if estimated_bytes >= file_size as usize {
        0
    } else {
        file_size - estimated_bytes as u64
    };

    file.seek(SeekFrom::Start(start_pos))?;

    let reader = BufReader::new(file);
    let mut all_lines: Vec<String> = reader.lines().collect::<std::io::Result<_>>()?;

    // Keep only the last n lines
    if all_lines.len() > n {
        all_lines = all_lines.split_off(all_lines.len() - n);
    }

    Ok(all_lines)
}
