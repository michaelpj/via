use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{self, Write, BufRead, BufReader};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;

use crate::session;

/// Write to a session's stdin pipe
pub fn write_session(session_name: &str, args: &[String]) -> Result<()> {
    let stdin_path = session::stdin_path(session_name)?;

    if !stdin_path.exists() {
        anyhow::bail!("no stdin at {} (is the session running?)", stdin_path.display());
    }

    // Open non-blocking: a blocking open of a FIFO with no reader hangs
    // forever, which is what a client would hit on a stale session (e.g.
    // one whose supervisor was SIGKILLed). Fail fast instead.
    let file = match OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&stdin_path)
    {
        Err(e) if e.raw_os_error() == Some(libc::ENXIO) => {
            anyhow::bail!(
                "nothing is reading {} — the session appears to be dead; remove {} and start a new one",
                stdin_path.display(),
                session::session_path(session_name)?.display()
            );
        }
        other => other.with_context(|| format!("failed to open {}", stdin_path.display()))?,
    };

    // Restore blocking mode so large writes wait for the reader instead of
    // failing with EAGAIN when the pipe buffer fills.
    unsafe {
        let fd = file.as_raw_fd();
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
    }
    let mut file = file;

    if !args.is_empty() {
        // Write args as a single line
        let line = args.join(" ");
        writeln!(file, "{}", line)
            .with_context(|| "failed to write to session stdin")?;
    } else {
        // Read from stdin and forward to the pipe
        let stdin = io::stdin();
        let reader = BufReader::new(stdin);

        for line in reader.lines() {
            let line = line.with_context(|| "failed to read from stdin")?;
            writeln!(file, "{}", line)
                .with_context(|| "failed to write to session stdin")?;
        }
    }

    Ok(())
}
