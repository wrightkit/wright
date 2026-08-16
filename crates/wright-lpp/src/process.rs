//! The stdio process adapter: spawn, observe, and terminate a long-running
//! LPP provider binary.
//!
//! LPP is a process boundary: the client spawns the provider as a child
//! process and communicates over its standard input and output. The
//! provider's standard error is reserved for human-readable logging and
//! passed through to the client's own stderr; it never carries protocol
//! messages.

use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use crate::error::ProviderError;

/// A spawned LPP provider child process.
pub struct ChildProcess {
    child: Child,
}

impl ChildProcess {
    /// Spawn `command` with `args`, stdin/stdout piped and stderr inherited.
    pub fn spawn(command: &Path, args: &[String]) -> Result<ChildProcess, ProviderError> {
        let child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| ProviderError::Spawn {
                message: format!("cannot spawn provider '{}': {error}", command.display()),
            })?;
        Ok(ChildProcess { child })
    }

    /// Take the piped stdin handle (once).
    pub fn take_stdin(&mut self) -> ChildStdin {
        self.child
            .stdin
            .take()
            .expect("the provider stdin handle was already taken")
    }

    /// Take the piped stdout handle (once).
    pub fn take_stdout(&mut self) -> ChildStdout {
        self.child
            .stdout
            .take()
            .expect("the provider stdout handle was already taken")
    }

    /// The provider's exit code when it has already exited (reaping it),
    /// else `None`.
    pub fn try_status(&mut self) -> Option<i32> {
        self.child.try_wait().ok().flatten().map(exit_code)
    }

    /// Block until the provider exits and return its exit code.
    pub fn wait(&mut self) -> Option<i32> {
        self.child.wait().ok().map(exit_code)
    }

    /// Kill the provider immediately.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Graceful bounded termination: wait up to 5 seconds for the provider
    /// to exit on its own (after its stdin reached end-of-file), then kill
    /// it. Returns the observed exit code.
    pub fn terminate(&mut self) -> Option<i32> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.try_status() {
                return Some(status);
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.kill();
        self.wait()
    }
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}
