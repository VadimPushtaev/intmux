use std::ffi::OsString;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::SESSION_NAME;
use crate::model::{CommandSpec, IntmuxError, WindowId, shell_join, shell_quote};
use crate::reuse::{REUSE_WINDOW_OPTION, ReuseKey};
use crate::tmux::client::TmuxClient;
use crate::tmux::process::TmuxRunner;
use crate::tmux::sticky::{ReuseResolution, StickyWindow};

impl<R: TmuxRunner> TmuxClient<'_, R> {
    pub(super) fn clear_stale_matches(
        &mut self,
        stale_matches: &[WindowId],
    ) -> Result<(), IntmuxError> {
        for stale_match in stale_matches {
            self.unset_window_option(stale_match.as_str(), REUSE_WINDOW_OPTION)?;
        }
        Ok(())
    }

    pub(super) fn default_shell_name(&mut self) -> Result<String, IntmuxError> {
        let output = self.execute_checked(
            "read tmux default shell",
            &[
                OsString::from("show-options"),
                OsString::from("-v"),
                OsString::from("-g"),
                OsString::from("default-shell"),
            ],
        )?;
        let shell = output.trimmed_stdout();
        let basename = Path::new(&shell)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or(IntmuxError::UnexpectedTmuxOutput {
                context: "read tmux default shell",
                details: format!("invalid default shell path: {shell:?}"),
            })?;
        Ok(String::from(basename))
    }

    pub(super) fn list_matching_windows(
        &mut self,
        reuse_key: &ReuseKey,
    ) -> Result<Vec<StickyWindow>, IntmuxError> {
        let output = self.execute_checked(
            "list matching tmux windows",
            &[
                OsString::from("list-windows"),
                OsString::from("-t"),
                OsString::from(SESSION_NAME.as_str()),
                OsString::from("-f"),
                OsString::from(format!(
                    "#{{==:#{{{REUSE_WINDOW_OPTION}}},{}}}",
                    reuse_key.as_str()
                )),
                OsString::from("-F"),
                OsString::from(
                    "#{window_id}\t#{window_index}\t#{window_panes}\t#{pane_id}\t#{pane_dead}\t#{pane_current_command}",
                ),
            ],
        )?;
        let stdout = output.trimmed_stdout();
        if stdout.is_empty() {
            return Ok(Vec::new());
        }

        stdout
            .lines()
            .map(StickyWindow::parse)
            .collect::<Result<Vec<_>, _>>()
    }

    pub(super) fn resolve_reuse_window(
        &mut self,
        reuse_key: &ReuseKey,
    ) -> Result<ReuseResolution, IntmuxError> {
        let mut matches = self.list_matching_windows(reuse_key)?;
        if matches.is_empty() {
            return Ok(ReuseResolution::Create(Vec::new()));
        }

        matches.sort_by_key(StickyWindow::window_index);
        let default_shell = self.default_shell_name()?;
        let canonical = matches.remove(0);
        let mut stale_matches = matches
            .into_iter()
            .map(|sticky_window| sticky_window.window_id().clone())
            .collect::<Vec<_>>();

        if canonical.is_reusable(&default_shell) {
            Ok(ReuseResolution::Reuse(
                canonical.into_target(),
                stale_matches,
            ))
        } else {
            stale_matches.insert(0, canonical.window_id().clone());
            Ok(ReuseResolution::Create(stale_matches))
        }
    }

    pub(super) fn reuse_shell(
        &mut self,
        pane_id: &str,
        spec: &CommandSpec,
    ) -> Result<(), IntmuxError> {
        self.send_key(pane_id, "C-c", "reset tmux shell")?;
        self.send_literal(
            pane_id,
            &format!("cd -- {}", shell_quote(spec.cwd().as_os_str())),
            "send tmux shell cd command",
        )?;
        self.send_key(pane_id, "C-m", "execute tmux shell cd command")?;
        self.send_command_line(pane_id, spec)
    }

    pub(super) fn send_command_line(
        &mut self,
        pane_id: &str,
        spec: &CommandSpec,
    ) -> Result<(), IntmuxError> {
        let command_line = shell_join(spec.argv());
        self.send_literal(pane_id, &command_line, "send command to tmux shell")?;
        self.send_key(pane_id, "C-m", "execute command in tmux shell")
    }

    pub(super) fn send_key(
        &mut self,
        pane_id: &str,
        key: &str,
        context: &'static str,
    ) -> Result<(), IntmuxError> {
        self.execute_checked(
            context,
            &[
                OsString::from("send-keys"),
                OsString::from("-t"),
                OsString::from(pane_id),
                OsString::from(key),
            ],
        )?;
        Ok(())
    }

    pub(super) fn send_literal(
        &mut self,
        pane_id: &str,
        command_line: &str,
        context: &'static str,
    ) -> Result<(), IntmuxError> {
        self.execute_checked(
            context,
            &[
                OsString::from("send-keys"),
                OsString::from("-t"),
                OsString::from(pane_id),
                OsString::from("-l"),
                OsString::from(command_line),
            ],
        )?;
        Ok(())
    }

    pub(super) fn set_window_option(
        &mut self,
        window_id: &str,
        option: &'static str,
        value: &str,
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

    pub(super) fn unset_window_option(
        &mut self,
        window_id: &str,
        option: &'static str,
    ) -> Result<(), IntmuxError> {
        self.execute_checked(
            "clear tmux window option",
            &[
                OsString::from("set-window-option"),
                OsString::from("-u"),
                OsString::from("-t"),
                OsString::from(window_id),
                OsString::from(option),
            ],
        )?;
        Ok(())
    }

    pub(super) fn wait_for_live_pane(&mut self, pane_id: &str) -> Result<(), IntmuxError> {
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
