use std::ffi::OsString;
use std::io;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

use crate::SESSION_NAME;
use crate::model::{
    CommandSpec, CreateTarget, IntmuxError, RunOptions, parse_create_target, shell_join,
};

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
    fn failure_details(&self) -> String {
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

    fn is_success(&self) -> bool {
        self.status_code == Some(0)
    }

    fn trimmed_stdout(&self) -> String {
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

pub(crate) struct TmuxClient<'runner, R> {
    options: &'runner RunOptions,
    runner: &'runner mut R,
}

impl<'runner, R: TmuxRunner> TmuxClient<'runner, R> {
    pub(crate) fn new(runner: &'runner mut R, options: &'runner RunOptions) -> Self {
        Self { options, runner }
    }

    pub(crate) fn launch(&mut self, spec: &CommandSpec) -> Result<(), IntmuxError> {
        let target = if self.has_session()? {
            self.new_window(spec)?
        } else {
            self.new_session(spec)?
        };

        self.set_window_option(target.window_id().as_str(), "automatic-rename", "off")?;
        self.wait_for_live_pane(target.pane_id().as_str())?;
        self.send_command_line(target.pane_id().as_str(), spec)
    }

    fn execute(
        &mut self,
        context: &'static str,
        args: &[OsString],
    ) -> Result<ProcessOutput, IntmuxError> {
        let mut full_args = Vec::with_capacity(args.len() + 2);
        if let Some(socket_name) = self.options.socket_name() {
            full_args.push(OsString::from("-L"));
            full_args.push(OsString::from(socket_name));
        }
        full_args.extend(args.iter().cloned());

        self.runner.run(&full_args).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                IntmuxError::TmuxNotFound
            } else {
                IntmuxError::TmuxIo { context, source }
            }
        })
    }

    fn execute_checked(
        &mut self,
        context: &'static str,
        args: &[OsString],
    ) -> Result<ProcessOutput, IntmuxError> {
        let output = self.execute(context, args)?;
        if output.is_success() {
            Ok(output)
        } else {
            Err(IntmuxError::TmuxCommand {
                context,
                details: output.failure_details(),
            })
        }
    }

    fn has_session(&mut self) -> Result<bool, IntmuxError> {
        let output = self.execute(
            "probe tmux session",
            &[
                OsString::from("has-session"),
                OsString::from("-t"),
                OsString::from(SESSION_NAME.as_str()),
            ],
        )?;

        match output.status_code {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(IntmuxError::TmuxCommand {
                context: "probe tmux session",
                details: output.failure_details(),
            }),
        }
    }

    fn new_session(&mut self, spec: &CommandSpec) -> Result<CreateTarget, IntmuxError> {
        let output = self.execute_checked(
            "create tmux session",
            &[
                OsString::from("new-session"),
                OsString::from("-d"),
                OsString::from("-P"),
                OsString::from("-F"),
                OsString::from("#{window_id}\t#{pane_id}"),
                OsString::from("-s"),
                OsString::from(SESSION_NAME.as_str()),
                OsString::from("-n"),
                OsString::from(spec.window_name()),
                OsString::from("-c"),
                spec.cwd().as_os_str().to_os_string(),
            ],
        )?;
        parse_create_target(&output.trimmed_stdout(), "create tmux session")
    }

    fn new_window(&mut self, spec: &CommandSpec) -> Result<CreateTarget, IntmuxError> {
        let output = self.execute_checked(
            "create tmux window",
            &[
                OsString::from("new-window"),
                OsString::from("-d"),
                OsString::from("-P"),
                OsString::from("-F"),
                OsString::from("#{window_id}\t#{pane_id}"),
                OsString::from("-t"),
                OsString::from(SESSION_NAME.as_str()),
                OsString::from("-n"),
                OsString::from(spec.window_name()),
                OsString::from("-c"),
                spec.cwd().as_os_str().to_os_string(),
            ],
        )?;
        parse_create_target(&output.trimmed_stdout(), "create tmux window")
    }

    fn send_command_line(&mut self, pane_id: &str, spec: &CommandSpec) -> Result<(), IntmuxError> {
        let command_line = shell_join(spec.argv());
        self.execute_checked(
            "send command to tmux shell",
            &[
                OsString::from("send-keys"),
                OsString::from("-t"),
                OsString::from(pane_id),
                OsString::from("-l"),
                OsString::from(command_line),
            ],
        )?;
        self.execute_checked(
            "execute command in tmux shell",
            &[
                OsString::from("send-keys"),
                OsString::from("-t"),
                OsString::from(pane_id),
                OsString::from("C-m"),
            ],
        )?;
        Ok(())
    }

    fn set_window_option(
        &mut self,
        window_id: &str,
        option: &'static str,
        value: &'static str,
    ) -> Result<(), IntmuxError> {
        self.execute_checked(
            "configure tmux window",
            &[
                OsString::from("set-window-option"),
                OsString::from("-t"),
                OsString::from(window_id),
                OsString::from(option),
                OsString::from(value),
            ],
        )?;
        Ok(())
    }

    fn wait_for_live_pane(&mut self, pane_id: &str) -> Result<(), IntmuxError> {
        const MAX_ATTEMPTS: usize = 20;
        const POLL_DELAY: Duration = Duration::from_millis(25);

        for _attempt in 0..MAX_ATTEMPTS {
            let output = self.execute_checked(
                "wait for tmux shell",
                &[
                    OsString::from("display-message"),
                    OsString::from("-p"),
                    OsString::from("-t"),
                    OsString::from(pane_id),
                    OsString::from("#{pane_dead}\t#{pane_current_command}"),
                ],
            )?;
            let status = output.trimmed_stdout();
            let mut parts = status.splitn(2, '\t');
            let pane_dead = parts.next().unwrap_or_default();
            let pane_command = parts.next().unwrap_or_default();
            if pane_dead == "0" && !pane_command.trim().is_empty() {
                thread::sleep(POLL_DELAY);
                return Ok(());
            }
            thread::sleep(POLL_DELAY);
        }

        Err(IntmuxError::UnexpectedTmuxOutput {
            context: "wait for tmux shell",
            details: format!("pane {pane_id} did not become a live shell in time"),
        })
    }
}
