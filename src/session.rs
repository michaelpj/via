use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;

/// Get the base directory for sessions, with fallback logic
pub fn base_dir() -> Result<PathBuf> {
    let default_base = PathBuf::from("/run/via");

    // Check REPLS_DIR environment variable first
    let base = if let Ok(repls_dir) = env::var("REPLS_DIR") {
        PathBuf::from(repls_dir)
    } else {
        default_base
    };

    // Try to create the directory if it doesn't exist
    if !base.exists() {
        if fs::create_dir_all(&base).is_ok() {
            return Ok(base);
        }

        // Fallback to XDG_RUNTIME_DIR or /tmp
        let fallback = if let Ok(xdg) = env::var("XDG_RUNTIME_DIR") {
            PathBuf::from(xdg).join("via")
        } else {
            PathBuf::from("/tmp/via")
        };

        fs::create_dir_all(&fallback)
            .with_context(|| format!("can't create {}", fallback.display()))?;

        return Ok(fallback);
    }

    Ok(base)
}

/// List all sessions (directories in base dir)
pub fn list_sessions(simple: bool) -> Result<()> {
    let base = base_dir()?;

    let mut sessions = Vec::new();

    if let Ok(entries) = fs::read_dir(&base) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        let session_dir = entry.path();

                        // Try to read metadata
                        let command = fs::read_to_string(session_dir.join("command"))
                            .ok();
                        let cwd = fs::read_to_string(session_dir.join("cwd"))
                            .ok();

                        // Try to detect current prompt from stdout
                        let prompt = detect_prompt(&session_dir);

                        let status = session_status(&session_dir);

                        sessions.push((name.to_string(), status, command, cwd, prompt));
                    }
                }
            }
        }
    }

    sessions.sort_by(|a, b| a.0.cmp(&b.0));

    if simple {
        // Simple format: just session names
        for (session, _, _, _, _) in sessions {
            println!("{}", session);
        }
    } else {
        // Table format
        if sessions.is_empty() {
            return Ok(());
        }

        // Calculate column widths
        let max_session_len = sessions.iter()
            .map(|(s, _, _, _, _)| s.len())
            .max()
            .unwrap_or(7)
            .max(7); // "Session" header

        let max_status_len = sessions.iter()
            .map(|(_, s, _, _, _)| s.len())
            .max()
            .unwrap_or(6)
            .max(6); // "Status" header

        let max_prompt_len = sessions.iter()
            .map(|(_, _, _, _, p)| p.as_ref().map(|s| s.len()).unwrap_or(0))
            .max()
            .unwrap_or(11)
            .max(11); // "Prompt Line" header

        let max_cwd_len = sessions.iter()
            .map(|(_, _, _, c, _)| c.as_ref().map(|s| s.trim().len()).unwrap_or(0))
            .max()
            .unwrap_or(17)
            .max(17); // "Working Directory" header

        // Print header
        println!("{:<width_session$}  {:<width_status$}  {:<width_prompt$}  {:<width_cwd$}  Command",
                 "Session", "Status", "Prompt Line", "Working Directory",
                 width_session = max_session_len,
                 width_status = max_status_len,
                 width_prompt = max_prompt_len,
                 width_cwd = max_cwd_len);

        // Print sessions
        for (session, status, command, cwd, prompt) in sessions {
            let prompt_str = prompt.as_deref().unwrap_or("");
            let cwd_str = cwd.as_ref().map(|c| c.trim()).unwrap_or("");
            let cmd_str = command.as_ref().map(|c| c.trim()).unwrap_or("");

            println!("{:<width_session$}  {:<width_status$}  {:<width_prompt$}  {:<width_cwd$}  {}",
                     session, status, prompt_str, cwd_str, cmd_str,
                     width_session = max_session_len,
                     width_status = max_status_len,
                     width_prompt = max_prompt_len,
                     width_cwd = max_cwd_len);
        }
    }

    Ok(())
}

/// Whether a session's recorded teetty process is still running.
///
/// "live" / "stale" when the session has a pid file; "?" for sessions
/// created by older versions of via that didn't record a pid.
fn session_status(session_dir: &std::path::Path) -> &'static str {
    let pid = fs::read_to_string(session_dir.join("pid"))
        .ok()
        .and_then(|s| s.trim().parse::<libc::pid_t>().ok());
    match pid {
        None => "?",
        Some(pid) => {
            // Signal 0 probes for existence without sending anything. EPERM
            // means the process exists but belongs to another user.
            let alive = unsafe { libc::kill(pid, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
            if alive { "live" } else { "stale" }
        }
    }
}

/// Detect the current prompt from the stdout file (show last line, up to 20 chars)
fn detect_prompt(session_dir: &std::path::Path) -> Option<String> {
    let stdout_path = session_dir.join("stdout");

    // Read the last ~1KB of the file to find the prompt
    if let Ok(mut file) = fs::File::open(&stdout_path) {
        use std::io::{Read, Seek, SeekFrom};

        let file_len = file.seek(SeekFrom::End(0)).ok()?;
        if file_len == 0 {
            return None;
        }

        let read_size = std::cmp::min(1024, file_len);
        file.seek(SeekFrom::End(-(read_size as i64))).ok()?;

        let mut buffer = vec![0u8; read_size as usize];
        file.read_exact(&mut buffer).ok()?;

        // Process terminal output to clean escape sequences
        let content = crate::prompt::process_terminal_output(&buffer).ok()?;

        // Get the last line
        let last_line = content.lines().last()?;

        // Truncate to 20 characters if longer
        if last_line.len() > 20 {
            return Some(format!("{}...", &last_line[..20]));
        } else if !last_line.is_empty() {
            return Some(last_line.to_string());
        }
    }

    None
}

/// Get the path for a specific session
pub fn session_path(session: &str) -> Result<PathBuf> {
    let base = base_dir()?;
    Ok(base.join(session))
}

/// Get the stdin pipe path for a session
pub fn stdin_path(session: &str) -> Result<PathBuf> {
    Ok(session_path(session)?.join("stdin"))
}

/// Get the stdout file path for a session
pub fn stdout_path(session: &str) -> Result<PathBuf> {
    Ok(session_path(session)?.join("stdout"))
}

/// Check if a session exists (has both stdin and stdout)
#[allow(dead_code)]
pub fn session_exists(session: &str) -> Result<bool> {
    let stdin = stdin_path(session)?;
    let stdout = stdout_path(session)?;
    Ok(stdin.exists() && stdout.exists())
}

/// Get the delim file path for a session
pub fn delim_path(session: &str) -> Result<PathBuf> {
    Ok(session_path(session)?.join("delim"))
}

/// Read the stored delimiter for a session, if any
pub fn get_delim(session: &str) -> Result<Option<String>> {
    let path = delim_path(session)?;
    match fs::read_to_string(&path) {
        Ok(s) => Ok(Some(s.trim().to_string())),
        Err(_) => Ok(None),
    }
}

/// Resolve a flag value: if the next arg exists and doesn't start with "--",
/// use it as the explicit value. Otherwise fall back to the stored session delim.
/// Returns (value, number_of_args_consumed).
pub fn resolve_delim(session: &str, args: &[String], i: usize) -> Result<(String, usize)> {
    let next = args.get(i + 1);
    let is_bare = match next {
        None => true,
        Some(s) => s.starts_with("--"),
    };

    if is_bare {
        match get_delim(session)? {
            Some(d) => Ok((d, 1)),
            None => anyhow::bail!("no stored delimiter for session '{}' (use --delim VALUE with 'via run')", session),
        }
    } else {
        Ok((next.unwrap().clone(), 2))
    }
}
