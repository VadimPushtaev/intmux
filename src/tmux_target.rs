use std::fmt;

use crate::model::IntmuxError;

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

    Ok(CreateTarget::new(
        PaneId::parse(pane_id)?,
        WindowId::parse(window_id)?,
    ))
}
