use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Runtime options used by the library entry points.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunOptions {
    reuse_window: bool,
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

    /// Enables reuse of a previously tagged tmux window for the same command and working directory.
    #[must_use]
    pub fn with_reuse_window(mut self) -> Self {
        self.reuse_window = true;
        self
    }

    pub(crate) fn reuse_window(&self) -> bool {
        self.reuse_window
    }

    pub(crate) fn socket_name(&self) -> Option<&str> {
        self.socket_name.as_ref().map(SocketName::as_str)
    }
}

/// Configuration validation failures for non-CLI library options.
#[non_exhaustive]
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
    argv: Vec<OsString>,
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

        Ok(Self {
            window_name: derive_window_name(executable),
            argv,
            cwd,
        })
    }

    pub(crate) fn argv(&self) -> &[OsString] {
        &self.argv
    }

    pub(crate) fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub(crate) fn window_name(&self) -> &str {
        &self.window_name
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
pub(crate) struct SessionName(pub(crate) &'static str);

impl SessionName {
    pub(crate) const fn as_str(self) -> &'static str {
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
pub(crate) struct WindowId(String);

impl WindowId {
    pub(crate) fn parse(raw: &str) -> Result<Self, IntmuxError> {
        parse_tmux_id(raw, '@', "window id").map(Self)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PaneId(String);

impl PaneId {
    pub(crate) fn parse(raw: &str) -> Result<Self, IntmuxError> {
        parse_tmux_id(raw, '%', "pane id").map(Self)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CreateTarget {
    pane_id: PaneId,
    window_id: WindowId,
}

impl CreateTarget {
    pub(crate) fn new(pane_id: PaneId, window_id: WindowId) -> Self {
        Self { pane_id, window_id }
    }

    pub(crate) fn pane_id(&self) -> &PaneId {
        &self.pane_id
    }

    pub(crate) fn window_id(&self) -> &WindowId {
        &self.window_id
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

pub(crate) fn parse_create_target(
    output: &str,
    context: &'static str,
) -> Result<CreateTarget, IntmuxError> {
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
        pane_id: PaneId::parse(pane_id)?,
        window_id: WindowId::parse(window_id)?,
    })
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
