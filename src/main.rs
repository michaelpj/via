use anyhow::{Context, Result};
use std::env;
use std::io::Write;
use std::process::exit;

mod session;
mod fifo;
mod tail;
mod prompt;

fn main() {
    exit(match run() {
        Ok(code) => code,
        Err(err) => {
            writeln!(std::io::stderr(), "via: {}", err).ok();
            1
        }
    })
}

fn run() -> Result<i32> {
    let mut args: Vec<String> = env::args().collect();

    // Remove program name
    args.remove(0);

    // Check for global flags first
    if !args.is_empty() {
        match args[0].as_str() {
            "--help" | "-h" => {
                show_usage_global();
                return Ok(0);
            }
            "--version" => {
                eprintln!("via {}", env!("CARGO_PKG_VERSION"));
                return Ok(0);
            }
            _ => {}
        }
    }

    if args.is_empty() {
        // via → list sessions (table format)
        session::list_sessions(false)?;
        return Ok(0);
    }

    // Check for --simple flag before other processing
    if args[0] == "--simple" {
        session::list_sessions(true)?;
        return Ok(0);
    }

    let first_arg = &args[0];

    if first_arg == "help" {
        // via help → show global usage
        show_usage_global();
        return Ok(0);
    }

    // via run [--delim D] -- <cmd> ... → auto-generate session name
    if first_arg == "run" {
        let remaining = &args[1..];
        let session_name = generate_session_name(remaining)?;
        eprintln!("[via] session: {}", session_name);
        cmd_run(&session_name, remaining)?;
        return Ok(0);
    }

    // First arg is the session name. `.` resolves to the current directory's
    // name, so `via . <command>` always targets the session for this checkout.
    let session_name = resolve_session_name(first_arg.clone())?;

    if args.len() < 2 {
        // via <session> with no args — use shorthand with piped stdin
        cmd_shorthand(&session_name, &[])?;
        return Ok(0);
    }

    let subcmd = &args[1];
    let remaining_args = &args[2..];

    match subcmd.as_str() {
        "help" => {
            show_session_usage(&session_name);
            Ok(0)
        }
        "run" => {
            cmd_run(&session_name, remaining_args)?;
            Ok(0)
        }
        "wait" => {
            cmd_wait(&session_name, remaining_args)?;
            Ok(0)
        }
        "write" => {
            cmd_write(&session_name, remaining_args)?;
            Ok(0)
        }
        "tail" => {
            cmd_tail(&session_name, remaining_args)?;
            Ok(0)
        }
        "path" => {
            cmd_path(&session_name)?;
            Ok(0)
        }
        _ => {
            // Only try shorthand if the session actually exists.
            // Otherwise it's likely a typo in the subcommand.
            let dir = session::session_path(&session_name)?;
            if !dir.exists() {
                anyhow::bail!("unknown session '{}' (try: via help)", session_name);
            }
            cmd_shorthand(&session_name, &args[1..])?;
            Ok(0)
        }
    }
}

/// Resolve a session name, mapping `.` to the current directory's name.
/// Everything else is returned unchanged. This keeps `via` git-agnostic: the
/// "session here" is simply the basename of the working directory.
fn resolve_session_name(name: String) -> Result<String> {
    if name == "." {
        let cwd = env::current_dir()?;
        cwd.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .ok_or_else(|| anyhow::anyhow!("cannot resolve '.': cwd has no basename"))
    } else {
        Ok(name)
    }
}

fn show_usage_global() {
    println!(r#"usage:
  via [--simple]                                          # list sessions (tabular format by default)
  via help                                                # this help
  via <session> help                                      # help for a specific session name
  via . <command>                                         # '.' names the session after the current directory
  via run [--delim 'PROMPT>'] [--bg] -- <cmd> ...         # start session with auto-generated name
  via <session> run [--delim 'PROMPT>'] [--bg] -- <cmd>   # start a named session running <cmd>
  via <session> wait [--until 'PROMPT>'] [--timeout N]    # wait for prompt (default: stored delim)
  via <session> [--delim 'PROMPT>'] [--timeout N] line    # write input and stream until delim

low-level usage:
  via <session> write [line...]                           # write (reads stdin if none)
  via <session> tail -n N                                 # tail last N lines
  via <session> tail -f [-n N]                            # follow output in real-time
  via <session> tail --since ['PROMPT>']                  # tail since last prompt (bare = stored delim)
  via <session> tail --delim ['PROMPT>']                  # last stanza (bare = stored delim)
  via <session> tail --until ['PROMPT>'] [--timeout N]    # stream until prompt (bare = stored delim)
  via <session> tail --since --until [--timeout N]        # stream from last prompt until next
  via <session> path                                      # show session path"#);
}

fn show_session_usage(session: &str) {
    println!(r#"usage for '{session}':
  via {session} help                                      # help for a specific session
  via {session} wait [--until 'PROMPT>'] [--timeout N]    # wait for prompt (default: stored delim)
  via {session} [--delim 'PROMPT>'] [--timeout N] line    # write input and stream until delim

low-level usage:
  via {session} write [line...]                           # write (reads stdin if none)
  via {session} tail -n N                                 # tail last N lines
  via {session} tail -f [-n N]                            # follow output in real-time
  via {session} tail --since ['PROMPT>']                  # tail since last prompt (bare = stored delim)
  via {session} tail --delim ['PROMPT>']                  # last stanza (bare = stored delim)
  via {session} tail --until ['PROMPT>'] [--timeout N]    # stream until prompt (bare = stored delim)
  via {session} path                                      # show session path"#);
}

/// Generate a session name like "cabal-00", "cabal-01" based on the command.
fn generate_session_name(args: &[String]) -> Result<String> {
    let cmd_name = args.iter()
        .position(|a| a == "--")
        .and_then(|pos| args.get(pos + 1))
        .map(|cmd| {
            std::path::Path::new(cmd)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("session")
                .to_string()
        })
        .unwrap_or_else(|| "session".to_string());

    let base = session::base_dir()?;
    for index in 0u32..100 {
        let name = format!("{}-{:02}", cmd_name, index);
        if !base.join(&name).exists() {
            return Ok(name);
        }
    }
    anyhow::bail!("too many sessions for '{}'", cmd_name)
}

fn cmd_run(session: &str, args: &[String]) -> Result<()> {
    // Parse flags before the "--" separator
    let separator_pos = args.iter().position(|a| a == "--");
    match separator_pos {
        None => anyhow::bail!("usage: via {} run [--delim DELIM] -- <command> [args...]", session),
        Some(pos) if pos + 1 >= args.len() => {
            anyhow::bail!("usage: via {} run [--delim DELIM] -- <command> [args...]", session)
        }
        _ => {}
    }
    let separator_pos = separator_pos.unwrap();
    let pre_args = &args[..separator_pos];
    let cmd_args = &args[separator_pos + 1..];

    // Parse flags from pre-separator args
    let mut delim: Option<String> = None;
    let mut background = false;
    {
        let mut i = 0;
        while i < pre_args.len() {
            match pre_args[i].as_str() {
                "--delim" => {
                    if i + 1 >= pre_args.len() {
                        anyhow::bail!("--delim requires a value");
                    }
                    delim = Some(pre_args[i + 1].clone());
                    i += 2;
                }
                "--background" | "--bg" => {
                    background = true;
                    i += 1;
                }
                _ => anyhow::bail!("unknown flag before '--': {}", pre_args[i]),
            }
        }
    }

    // Get session directory and create it
    let dir = session::session_path(session)?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create directory {}", dir.display()))?;

    // Write metadata files
    let command = cmd_args.join(" ");
    std::fs::write(dir.join("command"), &command)
        .with_context(|| "failed to write command metadata")?;

    if let Some(ref d) = delim {
        std::fs::write(dir.join("delim"), d)
            .with_context(|| "failed to write delim metadata")?;
    }

    if let Ok(cwd) = env::current_dir() {
        std::fs::write(dir.join("cwd"), cwd.to_string_lossy().as_bytes())
            .with_context(|| "failed to write cwd metadata")?;
    }

    let stdin_path = dir.join("stdin");
    let stdout_path = dir.join("stdout");

    // Print session info
    eprintln!("[via] dir: {}", dir.display());
    eprintln!("[via] stdin: {}", stdin_path.display());
    eprintln!("[via] stdout: {}", stdout_path.display());
    eprintln!("[via] launching: {}", command);

    // Build teetty command
    let mut cmd = std::process::Command::new("teetty");
    cmd.arg("-i").arg(&stdin_path)
       .arg("-o").arg(&stdout_path)
       .arg("--truncate")
       .arg("--");

    // Add the command and args
    for arg in cmd_args {
        cmd.arg(arg);
    }

    if background {
        // Detach teetty from our stdio so it doesn't consume the shell's
        // stdin or pollute stdout. teetty uses the FIFO and file instead.
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        // Fork before spawning teetty so the child process is teetty's
        // actual parent and can wait on it for cleanup.
        unsafe {
            match libc::fork() {
                -1 => anyhow::bail!("fork failed"),
                0 => {
                    // Child: detach from terminal, spawn teetty, wait, clean up
                    libc::setsid();
                    let mut child = match cmd.spawn() {
                        Ok(c) => c,
                        Err(_) => std::process::exit(1),
                    };
                    // Record the teetty pid so the session listing can tell
                    // live sessions from stale ones
                    let _ = std::fs::write(dir.join("pid"), child.id().to_string());
                    // If this supervisor is signalled, kill teetty (and with it
                    // the supervised command) so nothing is orphaned, then
                    // clean up the session directory.
                    let dir_for_cleanup = dir.clone();
                    let teetty_pid = child.id() as libc::pid_t;
                    ctrlc::set_handler(move || {
                        libc::kill(teetty_pid, libc::SIGTERM);
                        let _ = std::fs::remove_dir_all(&dir_for_cleanup);
                        std::process::exit(130);
                    }).ok();
                    let _ = child.wait();
                    let _ = std::fs::remove_dir_all(&dir);
                    std::process::exit(0);
                }
                _ => {}
            }
        }

        // Parent: poll until teetty has created the stdin FIFO and stdout file
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if stdin_path.exists() && stdout_path.exists() {
                break;
            }
            if std::time::Instant::now() > deadline {
                anyhow::bail!("teetty did not create stdin/stdout within 5s");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
    } else {
        // Run teetty in the foreground — blocks until the subprocess exits.
        let mut child = cmd.spawn()
            .with_context(|| "failed to execute teetty (is it installed?)")?;

        // Record the teetty pid so the session listing can tell live
        // sessions from stale ones
        let _ = std::fs::write(dir.join("pid"), child.id().to_string());

        // On SIGINT/SIGTERM/SIGHUP, kill teetty (and with it the supervised
        // command) so nothing is orphaned, then clean up the session directory.
        let dir_for_cleanup = dir.clone();
        let teetty_pid = child.id() as libc::pid_t;
        ctrlc::set_handler(move || {
            unsafe { libc::kill(teetty_pid, libc::SIGTERM); }
            let _ = std::fs::remove_dir_all(&dir_for_cleanup);
            std::process::exit(130);
        }).ok();

        let status = child.wait()
            .with_context(|| "failed to wait for teetty")?;

        // Cleanup directory after teetty exits
        std::fs::remove_dir_all(&dir).ok();

        if let Some(code) = status.code() {
            std::process::exit(code);
        } else {
            std::process::exit(1);
        }
    }
}

/// Wait for a prompt to appear in an already-running session's output.
/// Thin wrapper around tail --until with suppressed output.
/// `via <session> wait [--until PROMPT] [--timeout N]`
/// If --until is bare or omitted, uses stored session delim.
fn cmd_wait(session: &str, args: &[String]) -> Result<()> {
    let mut prompt: Option<String> = None;
    let mut timeout = tail::DEFAULT_TIMEOUT;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--until" => {
                let (val, consumed) = session::resolve_delim(session, args, i)?;
                prompt = Some(val);
                i += consumed;
            }
            "--timeout" => {
                if i + 1 >= args.len() {
                    anyhow::bail!("--timeout requires a number");
                }
                timeout = args[i + 1].parse()
                    .with_context(|| format!("invalid timeout: {}", args[i + 1]))?;
                i += 2;
            }
            other => {
                anyhow::bail!("unexpected argument: {} (use --until PROMPT to specify a prompt)", other);
            }
        }
    }

    // Fall back to stored delim if no prompt specified
    let prompt = match prompt {
        Some(p) => p,
        None => session::get_delim(session)?
            .ok_or_else(|| anyhow::anyhow!("no delimiter: use --until PROMPT or set --delim on 'via run'"))?,
    };

    tail::follow_until(session, &prompt, timeout, 0, &mut std::io::sink())?;
    eprintln!("[via] ready (prompt detected)");
    Ok(())
}

fn cmd_write(session: &str, args: &[String]) -> Result<()> {
    fifo::write_session(session, args)
}

fn cmd_tail(session: &str, args: &[String]) -> Result<()> {
    tail::tail_session(session, args)
}

fn cmd_path(session: &str) -> Result<()> {
    let path = session::session_path(session)?;
    println!("{}", path.display());
    Ok(())
}

/// Shorthand: via <session> [--delim D] [--timeout N] line...
/// Uses stored delim if --delim not provided.
fn cmd_shorthand(session: &str, args: &[String]) -> Result<()> {
    let mut delim: Option<String> = None;
    let mut timeout = tail::DEFAULT_TIMEOUT;
    let mut input_args: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--delim" => {
                let (val, consumed) = session::resolve_delim(session, args, i)?;
                delim = Some(val);
                i += consumed;
            }
            "--timeout" => {
                if i + 1 >= args.len() {
                    anyhow::bail!("--timeout requires a number");
                }
                timeout = args[i + 1].parse()
                    .with_context(|| format!("invalid timeout: {}", args[i + 1]))?;
                i += 2;
            }
            _ => {
                input_args.push(args[i].clone());
                i += 1;
            }
        }
    }

    // Resolve delimiter
    let prompt = match delim {
        Some(d) => d,
        None => session::get_delim(session)?
            .ok_or_else(|| anyhow::anyhow!("unknown subcommand or no stored delimiter for '{}' (try: via {} help)", session, session))?,
    };

    // 1. Check that the prompt is at the end of the output
    prompt::check_prompt_ready(session, &prompt)?;

    // 2. Record current file position before writing
    let stdout_path = session::stdout_path(session)?;
    let pos = {
        use std::io::{Seek, SeekFrom};
        let mut f = std::fs::File::open(&stdout_path)?;
        f.seek(SeekFrom::End(0))?
    };

    // 3. Write input (from args or stdin)
    fifo::write_session(session, &input_args)?;

    // 4. Stream output until the next prompt appears
    tail::follow_until(session, &prompt, timeout, pos, &mut std::io::stdout())
}

