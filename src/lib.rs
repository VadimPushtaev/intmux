#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(unused_lifetimes)]
#![deny(unused_qualifications)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::allow_attributes_without_reason)]
#![deny(clippy::exhaustive_enums)]
#![allow(
    clippy::module_name_repetitions,
    reason = "Rust modules often mirror the domain names of their contained types."
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "The public API is small and error variants already carry targeted messages."
)]
#![allow(
    clippy::must_use_candidate,
    reason = "The crate exposes constructors and helpers where pervasive must_use noise is not helpful."
)]
#![allow(
    clippy::similar_names,
    reason = "Tmux concepts such as pane, window, and session naturally produce similar identifiers."
)]

//! Launch commands inside a dedicated tmux session without disturbing clients.

#[cfg(not(unix))]
compile_error!("intmux currently supports Unix-like systems only.");

mod model;
mod reuse;
mod tmux;
mod tmux_target;

#[cfg(test)]
mod tests;

use std::env;
use std::ffi::OsString;

use clap::Parser;

pub(crate) use model::{CommandSpec, SessionName};
pub use model::{ConfigError, IntmuxError, RunOptions};
pub(crate) use tmux::TmuxRunner;

#[cfg(test)]
pub(crate) use model::shell_quote;

#[cfg(test)]
pub(crate) use tmux_target::parse_create_target;

#[cfg(test)]
pub(crate) use reuse::{compute_reuse_key, compute_shell_command_reuse_key};

#[cfg(test)]
pub(crate) use tmux::ProcessOutput;

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
    let run_options = merge_run_options(&cli, options);
    if let Some(shell_command) = cli.shell_command {
        launch_shell_command(shell_command, cwd, &run_options)
    } else {
        launch_command(cli.command, cwd, &run_options)
    }
}

/// Launches a command into the `intmux` tmux session from the given directory.
pub fn launch_command<I, T>(
    command: I,
    cwd: std::path::PathBuf,
    options: &RunOptions,
) -> Result<(), IntmuxError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let spec = CommandSpec::new(command, cwd)?;
    let mut runner = tmux::SystemTmuxRunner;
    launch_with_runner(&mut runner, &spec, options)
}

/// Launches a shell command line into the selected tmux session from the given directory.
pub fn launch_shell_command(
    command_line: impl Into<String>,
    cwd: std::path::PathBuf,
    options: &RunOptions,
) -> Result<(), IntmuxError> {
    let spec = CommandSpec::from_shell_command(command_line.into(), cwd)?;
    let mut runner = tmux::SystemTmuxRunner;
    launch_with_runner(&mut runner, &spec, options)
}

fn launch_with_runner<R: TmuxRunner>(
    runner: &mut R,
    spec: &CommandSpec,
    options: &RunOptions,
) -> Result<(), IntmuxError> {
    let mut tmux = tmux::TmuxClient::new(runner, options);
    tmux.launch(spec)
}

fn merge_run_options(cli: &Cli, options: &RunOptions) -> RunOptions {
    let mut run_options = if cli.reuse_window {
        options.clone().with_reuse_window()
    } else {
        options.clone()
    };

    if let Some(session_name) = cli.session.clone() {
        run_options = run_options.with_validated_session_name(session_name);
    }

    run_options
}

#[derive(Debug, Parser)]
#[command(
    name = "intmux",
    version,
    about = "Launch a command in a tmux session.",
    disable_help_subcommand = true,
    dont_collapse_args_in_usage = true,
    trailing_var_arg = true
)]
struct Cli {
    /// Target a specific tmux session instead of the default `intmux`.
    #[arg(long, value_name = "NAME", value_parser = parse_session_name_cli)]
    session: Option<SessionName>,

    /// Run a shell command line inside tmux without local shell parsing.
    #[arg(
        short = 'c',
        long = "shell-command",
        value_name = "COMMAND",
        conflicts_with = "command"
    )]
    shell_command: Option<String>,

    /// Reuse a previously tagged tmux window for the same command and working directory.
    #[arg(long)]
    reuse_window: bool,

    /// Command to run inside tmux. `--` is optional and only needed to disambiguate.
    #[arg(
        required_unless_present = "shell_command",
        num_args = 1..,
        allow_hyphen_values = true,
        value_name = "COMMAND [ARGS]..."
    )]
    command: Vec<OsString>,
}

fn parse_session_name_cli(value: &str) -> Result<SessionName, String> {
    SessionName::new(String::from(value)).map_err(|error| error.to_string())
}
