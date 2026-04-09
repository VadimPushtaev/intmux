use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Runtime options used by the library entry points.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunOptions {
    reuse_window: bool,
    session_name: Option<SessionName>,
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
            ..Self::default()
        })
    }

    /// Creates options that use a specific tmux session name.
    pub fn with_session_name(
        mut self,
        session_name: impl Into<String>,
    ) -> Result<Self, ConfigError> {
        self.session_name = Some(SessionName::new(session_name.into())?);
        Ok(self)
    }

    /// Enables reuse of a previously tagged tmux window for the same command and working directory.
    #[must_use]
    pub fn with_reuse_window(mut self) -> Self {
        self.reuse_window = true;
        self
    }

    pub(crate) fn reuse_window(&self) -> bool {
        self.reuse_window
    }

    pub(crate) fn session_name(&self) -> &str {
        self.session_name
            .as_ref()
            .map_or(SessionName::default_name(), SessionName::as_str)
    }

    pub(crate) fn socket_name(&self) -> Option<&str> {
        self.socket_name.as_ref().map(SocketName::as_str)
    }

    pub(crate) fn with_validated_session_name(mut self, session_name: SessionName) -> Self {
        self.session_name = Some(session_name);
        self
    }
}

/// Configuration validation failures for non-CLI library options.
#[non_exhaustive]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    /// The tmux session name was empty.
    #[error("tmux session name cannot be empty")]
    EmptySessionName,
    /// The tmux session name contained ':' which breaks tmux target parsing.
    #[error("tmux session name must not contain ':'")]
    SessionNameContainsColon,
    /// The tmux socket name was empty.
    #[error("tmux socket name cannot be empty")]
    EmptySocketName,
    /// The tmux socket name contained a path separator.
    #[error("tmux socket name must not contain '/'")]
    SocketNameContainsSeparator,
}

/// All recoverable failures produced by `intmux`.
#[non_exhaustive]
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
    /// `tmux` exited unsuccessfully for the requested action.
    #[error("failed to {context}: {details}")]
    TmuxCommand {
        /// The failed action.
        context: &'static str,
        /// A concise description of tmux's failure.
        details: String,
    },
    /// Starting `tmux` failed before it produced an exit status.
    #[error("failed to {context}: {source}")]
    TmuxIo {
        /// The failed action.
        context: &'static str,
        /// The underlying OS error.
        #[source]
        source: io::Error,
    },
    /// `tmux` was not found on `PATH`.
    #[error("tmux is not installed or not available on PATH")]
    TmuxNotFound,
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
pub(crate) struct CommandSpec {
    command: CommandInputOwned,
    cwd: PathBuf,
    window_name: String,
}

impl CommandSpec {
    pub(crate) fn new<I, T>(command: I, cwd: PathBuf) -> Result<Self, IntmuxError>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString>,
    {
        let argv: Vec<OsString> = command.into_iter().map(Into::into).collect();
        let Some(executable) = argv.first() else {
            return Err(IntmuxError::InvalidCommand(
                "command must contain at least one argument",
            ));
        };
        if executable.is_empty() {
            return Err(IntmuxError::InvalidCommand(
                "command executable must not be empty",
            ));
        }
        let window_name = derive_window_name(executable);

        Ok(Self {
            command: CommandInputOwned::Argv(argv),
            window_name,
            cwd,
        })
    }

    pub(crate) fn from_shell_command(
        command_line: String,
        cwd: PathBuf,
    ) -> Result<Self, IntmuxError> {
        if command_line.trim().is_empty() {
            return Err(IntmuxError::InvalidCommand(
                "shell command must not be empty",
            ));
        }

        Ok(Self {
            window_name: derive_shell_window_name(&command_line),
            command: CommandInputOwned::Shell(command_line),
            cwd,
        })
    }

    pub(crate) fn command_input(&self) -> CommandInput<'_> {
        match &self.command {
            CommandInputOwned::Argv(argv) => CommandInput::Argv(argv),
            CommandInputOwned::Shell(command_line) => CommandInput::Shell(command_line),
        }
    }

    pub(crate) fn rendered_command_line(&self) -> String {
        match self.command_input() {
            CommandInput::Argv(argv) => shell_join(argv),
            CommandInput::Shell(command_line) => String::from(command_line),
        }
    }

    pub(crate) fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub(crate) fn window_name(&self) -> &str {
        &self.window_name
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CommandInputOwned {
    Argv(Vec<OsString>),
    Shell(String),
}

pub(crate) enum CommandInput<'a> {
    Argv(&'a [OsString]),
    Shell(&'a str),
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

fn derive_shell_window_name(command_line: &str) -> String {
    let Some(first_word) = command_line.split_whitespace().next() else {
        return String::from("shell");
    };
    derive_window_name(OsStr::new(first_word))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionName(String);

impl SessionName {
    pub(crate) fn new(session_name: String) -> Result<Self, ConfigError> {
        if session_name.is_empty() {
            return Err(ConfigError::EmptySessionName);
        }
        if session_name.contains(':') {
            return Err(ConfigError::SessionNameContainsColon);
        }
        Ok(Self(session_name))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) const fn default_name() -> &'static str {
        "intmux"
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

pub(crate) fn shell_quote(argument: &OsStr) -> String {
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

pub(crate) fn shell_join(argv: &[OsString]) -> String {
    argv.iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}
