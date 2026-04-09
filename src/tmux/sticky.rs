use crate::model::IntmuxError;
use crate::tmux_target::{CreateTarget, PaneId, WindowId};

pub(super) enum ReuseResolution {
    Create(Vec<WindowId>),
    Reuse(CreateTarget, Vec<WindowId>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StickyWindow {
    pane_current_command: String,
    pane_dead: bool,
    pane_id: PaneId,
    window_id: WindowId,
    window_index: usize,
    window_panes: usize,
}

impl StickyWindow {
    pub(super) fn parse(line: &str) -> Result<Self, IntmuxError> {
        let mut parts = line.split('\t');
        let window_id =
            WindowId::parse(parts.next().ok_or(IntmuxError::UnexpectedTmuxOutput {
                context: "parse sticky tmux window",
                details: format!("missing window id in {line:?}"),
            })?)?;
        let window_index = parse_usize(parts.next(), "window index", line)?;
        let window_panes = parse_usize(parts.next(), "window panes", line)?;
        let pane_id = PaneId::parse(parts.next().ok_or(IntmuxError::UnexpectedTmuxOutput {
            context: "parse sticky tmux window",
            details: format!("missing pane id in {line:?}"),
        })?)?;
        let pane_dead = match parts.next() {
            Some("0") => false,
            Some("1") => true,
            Some(other) => {
                return Err(IntmuxError::UnexpectedTmuxOutput {
                    context: "parse sticky tmux window",
                    details: format!("invalid pane dead flag {other:?} in {line:?}"),
                });
            }
            None => {
                return Err(IntmuxError::UnexpectedTmuxOutput {
                    context: "parse sticky tmux window",
                    details: format!("missing pane dead flag in {line:?}"),
                });
            }
        };
        let pane_current_command = parts
            .next()
            .ok_or(IntmuxError::UnexpectedTmuxOutput {
                context: "parse sticky tmux window",
                details: format!("missing pane current command in {line:?}"),
            })?
            .to_owned();
        if parts.next().is_some() {
            return Err(IntmuxError::UnexpectedTmuxOutput {
                context: "parse sticky tmux window",
                details: format!("too many sticky tmux window fields in {line:?}"),
            });
        }

        Ok(Self {
            pane_current_command,
            pane_dead,
            pane_id,
            window_id,
            window_index,
            window_panes,
        })
    }

    pub(super) fn into_target(self) -> CreateTarget {
        let pane_id = self.pane_id;
        let window_id = self.window_id;
        CreateTarget::new(pane_id, window_id)
    }

    pub(super) fn is_reusable(&self, default_shell: &str) -> bool {
        self.window_panes == 1 && !self.pane_dead && self.pane_current_command == default_shell
    }

    pub(super) fn window_id(&self) -> &WindowId {
        &self.window_id
    }

    pub(super) fn window_index(&self) -> usize {
        self.window_index
    }
}

fn parse_usize(value: Option<&str>, label: &'static str, line: &str) -> Result<usize, IntmuxError> {
    let raw = value.ok_or(IntmuxError::UnexpectedTmuxOutput {
        context: "parse sticky tmux window",
        details: format!("missing {label} in {line:?}"),
    })?;
    raw.parse::<usize>()
        .map_err(|_| IntmuxError::UnexpectedTmuxOutput {
            context: "parse sticky tmux window",
            details: format!("invalid {label} {raw:?} in {line:?}"),
        })
}
