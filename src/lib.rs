#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(unused_lifetimes)]
#![deny(unused_qualifications)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::similar_names)]
#![allow(clippy::too_many_lines)]

//! Launch commands inside a dedicated tmux session without disturbing clients.

#[cfg(not(unix))]
compile_error!("intmux currently supports Unix-like systems only.");

use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

use clap::Parser;
use thiserror::Error;

const SESSION_NAME: SessionName = SessionName("intmux");

/// Runs the `intmux` CLI using `std::env::args_os()`.
pub fn try_main() -> Result<(), IntmuxError> {
    run_from_args(env::args_os(), &RunOptions::default())
}

/// Runs the `intmux` CLI from a caller-provided argv stream.
pub fn run_from_args<I, T>(args: I, options: &RunOptions) -> Result<(), IntmuxError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|error| IntmuxError::Cli(error.to_string()))?;
    let cwd = env::current_dir().map_err(IntmuxError::CurrentDirectory)?;
    launch_command(cli.command, cwd, options)
}

/// Launches a command into the `intmux` tmux session from the given directory.
pub fn launch_command<I, T>(
    command: I,
    cwd: PathBuf,
    options: &RunOptions,
) -> Result<(), IntmuxError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let spec = CommandSpec::new(command, cwd)?;
    let mut runner = SystemTmuxRunner;
    launch_with_runner(&mut runner, &spec, options)
}

fn launch_with_runner<R: TmuxRunner>(
    runner: &mut R,
    spec: &CommandSpec,
    options: &RunOptions,
) -> Result<(), IntmuxError> {
    let mut tmux = TmuxClient::new(runner, options);
    tmux.launch(spec)
}

#[derive(Debug, Parser)]
#[command(
    name = "intmux",
    version,
    about = "Launch a command in the tmux session named intmux.",
    disable_help_subcommand = true,
    dont_collapse_args_in_usage = true,
    trailing_var_arg = true
)]
struct Cli {
    /// Command to run inside tmux. `--` is optional and only needed to disambiguate.
    #[arg(
        required = true,
        num_args = 1..,
        allow_hyphen_values = true,
        value_name = "COMMAND [ARGS]..."
    )]
    command: Vec<OsString>,
}

/// Runtime options used by the library entry points.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunOptions {
    socket_name: Option<SocketName>,
}

impl RunOptions {
    /// Creates a new set of default runtime options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates options that use a specific tmux socket name.
    pub fn with_socket_name(socket_name: impl Into<String>) -> Result<Self, ConfigError> {
        Ok(Self {
            socket_name: Some(SocketName::new(socket_name.into())?),
        })
    }
}

/// Configuration validation failures for non-CLI library options.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    /// The tmux socket name was empty.
    #[error("tmux socket name cannot be empty")]
    EmptySocketName,
    /// The tmux socket name contained a path separator.
    #[error("tmux socket name must not contain '/'")]
    SocketNameContainsSeparator,
}

/// All recoverable failures produced by `intmux`.
#[derive(Debug, Error)]
pub enum IntmuxError {
    /// CLI argument parsing failed.
    #[error("{0}")]
    Cli(String),
    /// The current working directory could not be determined.
    #[error("failed to read current directory: {0}")]
    CurrentDirectory(#[source] io::Error),
    /// The command payload was structurally invalid.
    #[error("invalid command: {0}")]
    InvalidCommand(&'static str),
    /// `tmux` was not found on `PATH`.
    #[error("tmux is not installed or not available on PATH")]
    TmuxNotFound,
    /// Starting `tmux` failed before it produced an exit status.
    #[error("failed to {context}: {source}")]
    TmuxIo {
        /// The failed action.
        context: &'static str,
        /// The underlying OS error.
        #[source]
        source: io::Error,
    },
    /// `tmux` exited unsuccessfully for the requested action.
    #[error("failed to {context}: {details}")]
    TmuxCommand {
        /// The failed action.
        context: &'static str,
        /// A concise description of tmux's failure.
        details: String,
    },
    /// `tmux` produced output that did not match the expected machine format.
    #[error("unexpected tmux output while {context}: {details}")]
    UnexpectedTmuxOutput {
        /// The action that produced malformed output.
        context: &'static str,
        /// A concise description of the malformed data.
        details: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CommandSpec {
    argv: Vec<OsString>,
    cwd: PathBuf,
    window_name: String,
}

impl CommandSpec {
    fn new<I, T>(command: I, cwd: PathBuf) -> Result<Self, IntmuxError>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString>,
    {
        let argv: Vec<OsString> = command.into_iter().map(Into::into).collect();
        if argv.is_empty() {
            return Err(IntmuxError::InvalidCommand(
                "command must contain at least one argument",
            ));
        }
        if argv[0].is_empty() {
            return Err(IntmuxError::InvalidCommand(
                "command executable must not be empty",
            ));
        }

        Ok(Self {
            window_name: derive_window_name(&argv[0]),
            argv,
            cwd,
        })
    }
}

fn derive_window_name(command: &OsStr) -> String {
    let raw = Path::new(command)
        .file_name()
        .unwrap_or(command)
        .to_string_lossy();
    if raw.trim().is_empty() {
        String::from("cmd")
    } else {
        raw.into_owned()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SessionName(&'static str);

impl SessionName {
    fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SocketName(String);

impl SocketName {
    fn new(socket_name: String) -> Result<Self, ConfigError> {
        if socket_name.is_empty() {
            return Err(ConfigError::EmptySocketName);
        }
        if socket_name.contains('/') {
            return Err(ConfigError::SocketNameContainsSeparator);
        }
        Ok(Self(socket_name))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WindowId(String);

impl WindowId {
    fn parse(raw: &str) -> Result<Self, IntmuxError> {
        parse_tmux_id(raw, '@', "window id").map(Self)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PaneId(String);

impl PaneId {
    fn parse(raw: &str) -> Result<Self, IntmuxError> {
        parse_tmux_id(raw, '%', "pane id").map(Self)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn parse_tmux_id(raw: &str, prefix: char, label: &'static str) -> Result<String, IntmuxError> {
    let mut chars = raw.chars();
    let Some(head) = chars.next() else {
        return Err(IntmuxError::UnexpectedTmuxOutput {
            context: "parse tmux identifiers",
            details: format!("missing {label}"),
        });
    };
    if head != prefix || !chars.all(|character| character.is_ascii_digit()) {
        return Err(IntmuxError::UnexpectedTmuxOutput {
            context: "parse tmux identifiers",
            details: format!("invalid {label}: {raw:?}"),
        });
    }
    Ok(String::from(raw))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CreateTarget {
    window_id: WindowId,
    pane_id: PaneId,
}

trait TmuxRunner {
    fn run(&mut self, args: &[OsString]) -> io::Result<ProcessOutput>;
}

#[derive(Debug, Default)]
struct SystemTmuxRunner;

impl TmuxRunner for SystemTmuxRunner {
    fn run(&mut self, args: &[OsString]) -> io::Result<ProcessOutput> {
        let output = Command::new("tmux").args(args).output()?;
        Ok(ProcessOutput::from(output))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProcessOutput {
    status_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl ProcessOutput {
    fn is_success(&self) -> bool {
        self.status_code == Some(0)
    }

    fn trimmed_stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout)
            .trim_end_matches(['\n', '\r'])
            .to_owned()
    }

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

struct TmuxClient<'runner, R> {
    runner: &'runner mut R,
    options: &'runner RunOptions,
}

impl<'runner, R: TmuxRunner> TmuxClient<'runner, R> {
    fn new(runner: &'runner mut R, options: &'runner RunOptions) -> Self {
        Self { runner, options }
    }

    fn launch(&mut self, spec: &CommandSpec) -> Result<(), IntmuxError> {
        let target = if self.has_session(SESSION_NAME)? {
            self.new_window(spec)?
        } else {
            self.new_session(spec)?
        };

        self.set_window_option(target.window_id.as_str(), "automatic-rename", "off")?;
        self.wait_for_live_pane(target.pane_id.as_str())?;
        self.send_command_line(target.pane_id.as_str(), spec)
    }

    fn has_session(&mut self, session_name: SessionName) -> Result<bool, IntmuxError> {
        let output = self.execute(
            "probe tmux session",
            &[
                OsString::from("has-session"),
                OsString::from("-t"),
                OsString::from(session_name.as_str()),
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
                OsString::from(&spec.window_name),
                OsString::from("-c"),
                spec.cwd.as_os_str().to_os_string(),
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
                OsString::from(&spec.window_name),
                OsString::from("-c"),
                spec.cwd.as_os_str().to_os_string(),
            ],
        )?;
        parse_create_target(&output.trimmed_stdout(), "create tmux window")
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

    fn send_command_line(&mut self, pane_id: &str, spec: &CommandSpec) -> Result<(), IntmuxError> {
        let command_line = shell_join(&spec.argv);
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

    fn execute(
        &mut self,
        context: &'static str,
        args: &[OsString],
    ) -> Result<ProcessOutput, IntmuxError> {
        let mut full_args = Vec::with_capacity(args.len() + 2);
        if let Some(socket_name) = &self.options.socket_name {
            full_args.push(OsString::from("-L"));
            full_args.push(OsString::from(socket_name.as_str()));
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
}

fn shell_join(argv: &[OsString]) -> String {
    argv.iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(argument: &OsStr) -> String {
    let text = argument.to_string_lossy();
    if !text.is_empty()
        && text.chars().all(|character| {
            matches!(
                character,
                'A'..='Z' | 'a'..='z' | '0'..='9' | '/' | '.' | '_' | '-' | ':'
            )
        })
    {
        return text.into_owned();
    }

    let escaped = text.replace('\'', r"'\''");
    format!("'{escaped}'")
}

fn parse_create_target(output: &str, context: &'static str) -> Result<CreateTarget, IntmuxError> {
    let mut parts = output.split('\t');
    let window_id = parts.next().ok_or(IntmuxError::UnexpectedTmuxOutput {
        context,
        details: String::from("missing window id"),
    })?;
    let pane_id = parts.next().ok_or(IntmuxError::UnexpectedTmuxOutput {
        context,
        details: String::from("missing pane id"),
    })?;
    if parts.next().is_some() {
        return Err(IntmuxError::UnexpectedTmuxOutput {
            context,
            details: format!("expected two tab-separated fields, got {output:?}"),
        });
    }

    Ok(CreateTarget {
        window_id: WindowId::parse(window_id)?,
        pane_id: PaneId::parse(pane_id)?,
    })
}

#[cfg(test)]
mod tests;
