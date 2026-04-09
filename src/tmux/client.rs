use std::ffi::OsString;
use std::io;

use crate::SESSION_NAME;
use crate::model::{CommandSpec, CreateTarget, IntmuxError, RunOptions, parse_create_target};
use crate::reuse::{REUSE_WINDOW_OPTION, ReuseKey};
use crate::tmux::process::{ProcessOutput, TmuxRunner};
use crate::tmux::sticky::ReuseResolution;

pub(crate) struct TmuxClient<'runner, R> {
    options: &'runner RunOptions,
    runner: &'runner mut R,
}

impl<'runner, R: TmuxRunner> TmuxClient<'runner, R> {
    pub(crate) fn new(runner: &'runner mut R, options: &'runner RunOptions) -> Self {
        Self { options, runner }
    }

    pub(crate) fn launch(&mut self, spec: &CommandSpec) -> Result<(), IntmuxError> {
        let reuse_key = self
            .options
            .reuse_window()
            .then(|| ReuseKey::from_command_spec(spec));
        let session_exists = self.has_session()?;

        if session_exists && let Some(reuse_key) = &reuse_key {
            match self.resolve_reuse_window(reuse_key)? {
                ReuseResolution::Reuse(target, stale_matches) => {
                    self.clear_stale_matches(&stale_matches)?;
                    self.set_window_option(target.window_id().as_str(), "automatic-rename", "off")?;
                    self.set_window_option(
                        target.window_id().as_str(),
                        REUSE_WINDOW_OPTION,
                        reuse_key.as_str(),
                    )?;
                    return self.reuse_shell(target.pane_id().as_str(), spec);
                }
                ReuseResolution::Create(stale_matches) => {
                    self.clear_stale_matches(&stale_matches)?;
                }
            }
        }

        let target = if session_exists {
            self.new_window(spec)?
        } else {
            self.new_session(spec)?
        };
        self.set_window_option(target.window_id().as_str(), "automatic-rename", "off")?;
        if let Some(reuse_key) = &reuse_key {
            self.set_window_option(
                target.window_id().as_str(),
                REUSE_WINDOW_OPTION,
                reuse_key.as_str(),
            )?;
        }
        self.wait_for_live_pane(target.pane_id().as_str())?;
        self.send_command_line(target.pane_id().as_str(), spec)
    }

    pub(super) fn execute(
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

    pub(super) fn execute_checked(
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
}
