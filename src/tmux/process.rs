use std::ffi::OsString;
use std::io;
use std::process::{Command, Output};

pub(crate) trait TmuxRunner {
    fn run(&mut self, args: &[OsString]) -> io::Result<ProcessOutput>;
}

#[derive(Debug, Default)]
pub(crate) struct SystemTmuxRunner;

impl TmuxRunner for SystemTmuxRunner {
    fn run(&mut self, args: &[OsString]) -> io::Result<ProcessOutput> {
        let output = Command::new("tmux").args(args).output()?;
        Ok(ProcessOutput::from(output))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessOutput {
    pub(crate) status_code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

impl ProcessOutput {
    pub(crate) fn failure_details(&self) -> String {
        let stderr = String::from_utf8_lossy(&self.stderr).trim().to_owned();
        if !stderr.is_empty() {
            return stderr;
        }

        let stdout = String::from_utf8_lossy(&self.stdout).trim().to_owned();
        if !stdout.is_empty() {
            return stdout;
        }

        match self.status_code {
            Some(code) => format!("tmux exited with status {code}"),
            None => String::from("tmux terminated by signal"),
        }
    }

    pub(crate) fn is_success(&self) -> bool {
        self.status_code == Some(0)
    }

    pub(crate) fn trimmed_stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout)
            .trim_end_matches(['\n', '\r'])
            .to_owned()
    }
}

impl From<Output> for ProcessOutput {
    fn from(output: Output) -> Self {
        Self {
            status_code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}
