//! Integration tests for the real tmux orchestration path.

#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::similar_names)]
#![allow(clippy::too_many_lines)]

use std::error::Error;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use intmux::{RunOptions, launch_command};
use tempfile::TempDir;

#[test]
fn creates_session_with_live_shell_and_preserves_window_name() -> Result<(), Box<dyn Error>> {
    let harness = TmuxHarness::new()?;
    let workspace = tempfile::tempdir()?;

    launch_command(
        [OsString::from("touch"), OsString::from("first file")],
        workspace.path().to_path_buf(),
        &harness.options,
    )?;

    wait_for_path(&workspace.path().join("first file"))?;
    assert_eq!(harness.session_name()?, "intmux");
    assert_eq!(harness.window_count()?, 1);
    assert_eq!(harness.window_name(0)?, "touch");
    assert_eq!(
        harness.window_option(0, "automatic-rename")?,
        "automatic-rename off"
    );
    assert_eq!(harness.pane_dead(0)?, "0");
    harness.send_literal_command(0, "touch shell-still-works")?;
    wait_for_path(&workspace.path().join("shell-still-works"))?;

    Ok(())
}

#[test]
fn existing_session_gets_new_window_without_reusing_the_old_one() -> Result<(), Box<dyn Error>> {
    let harness = TmuxHarness::new()?;
    let workspace = tempfile::tempdir()?;

    launch_command(
        [OsString::from("touch"), OsString::from("first")],
        workspace.path().to_path_buf(),
        &harness.options,
    )?;
    launch_command(
        [OsString::from("touch"), OsString::from("second")],
        workspace.path().to_path_buf(),
        &harness.options,
    )?;

    wait_for_path(&workspace.path().join("first"))?;
    wait_for_path(&workspace.path().join("second"))?;
    assert_eq!(harness.window_count()?, 2);

    Ok(())
}

#[test]
fn command_arguments_with_spaces_are_passed_without_shell_quoting() -> Result<(), Box<dyn Error>> {
    let harness = TmuxHarness::new()?;
    let workspace = tempfile::tempdir()?;
    let target = workspace.path().join("file with spaces.txt");

    launch_command(
        [OsString::from("touch"), target.clone().into_os_string()],
        workspace.path().to_path_buf(),
        &harness.options,
    )?;

    wait_for_path(&target)?;
    Ok(())
}

#[test]
fn relative_paths_resolve_from_the_caller_directory() -> Result<(), Box<dyn Error>> {
    let harness = TmuxHarness::new()?;
    let workspace = tempfile::tempdir()?;

    launch_command(
        [OsString::from("touch"), OsString::from("relative-output")],
        workspace.path().to_path_buf(),
        &harness.options,
    )?;

    wait_for_path(&workspace.path().join("relative-output"))?;
    Ok(())
}

struct TmuxHarness {
    options: RunOptions,
    socket_name: String,
    _workspace: TempDir,
}

impl TmuxHarness {
    fn new() -> Result<Self, Box<dyn Error>> {
        let socket_name = unique_socket_name()?;
        let workspace = tempfile::tempdir()?;
        let options = RunOptions::with_socket_name(socket_name.clone())?;

        Ok(Self {
            options,
            socket_name,
            _workspace: workspace,
        })
    }

    fn session_name(&self) -> Result<String, Box<dyn Error>> {
        self.tmux_stdout(["list-sessions", "-F", "#{session_name}"])
    }

    fn window_count(&self) -> Result<usize, Box<dyn Error>> {
        let raw = self.tmux_stdout(["list-windows", "-t", "intmux", "-F", "#{window_id}"])?;
        Ok(raw.lines().count())
    }

    fn window_name(&self, index: usize) -> Result<String, Box<dyn Error>> {
        let window_id = self.window_id(index)?;
        self.tmux_stdout(["display-message", "-p", "-t", &window_id, "#{window_name}"])
    }

    fn window_option(&self, index: usize, option: &str) -> Result<String, Box<dyn Error>> {
        let window_id = self.window_id(index)?;
        self.tmux_stdout(["show-options", "-w", "-t", &window_id, option])
    }

    fn pane_dead(&self, index: usize) -> Result<String, Box<dyn Error>> {
        let pane_id = self.pane_id(index)?;
        self.tmux_stdout(["display-message", "-p", "-t", &pane_id, "#{pane_dead}"])
    }

    fn send_literal_command(&self, index: usize, command: &str) -> Result<(), Box<dyn Error>> {
        let pane_id = self.pane_id(index)?;
        self.tmux_success(["send-keys", "-t", &pane_id, "-l", command])?;
        self.tmux_success(["send-keys", "-t", &pane_id, "C-m"])?;
        Ok(())
    }

    fn window_id(&self, index: usize) -> Result<String, Box<dyn Error>> {
        let raw = self.tmux_stdout(["list-windows", "-t", "intmux", "-F", "#{window_id}"])?;
        let ids: Vec<&str> = raw.lines().collect();
        ids.get(index)
            .map(|id| (*id).to_owned())
            .ok_or_else(|| format!("missing window at index {index}").into())
    }

    fn pane_id(&self, index: usize) -> Result<String, Box<dyn Error>> {
        let raw = self.tmux_stdout(["list-panes", "-t", "intmux", "-F", "#{pane_id}"])?;
        let ids: Vec<&str> = raw.lines().collect();
        ids.get(index)
            .map(|id| (*id).to_owned())
            .ok_or_else(|| format!("missing pane at index {index}").into())
    }

    fn tmux_stdout<const N: usize>(&self, args: [&str; N]) -> Result<String, Box<dyn Error>> {
        let output = self.tmux_output(args)?;
        ensure_tmux_success(args.as_slice(), &output)?;
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    fn tmux_success<const N: usize>(&self, args: [&str; N]) -> Result<(), Box<dyn Error>> {
        let output = self.tmux_output(args)?;
        ensure_tmux_success(args.as_slice(), &output)?;
        Ok(())
    }

    fn tmux_output<const N: usize>(
        &self,
        args: [&str; N],
    ) -> Result<std::process::Output, Box<dyn Error>> {
        let output = Command::new("tmux")
            .arg("-L")
            .arg(&self.socket_name)
            .args(args)
            .output()?;
        Ok(output)
    }
}

impl Drop for TmuxHarness {
    fn drop(&mut self) {
        let _ignored = Command::new("tmux")
            .arg("-L")
            .arg(&self.socket_name)
            .arg("kill-server")
            .output();
    }
}

fn unique_socket_name() -> Result<String, Box<dyn Error>> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(format!(
        "intmux-test-{}-{}",
        std::process::id(),
        elapsed.as_nanos()
    ))
}

fn wait_for_path(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if path.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(format!("timed out waiting for {}", path.display()).into())
}

fn ensure_tmux_success(args: &[&str], output: &std::process::Output) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(format!("tmux {args:?} failed: {stderr}").into())
}
