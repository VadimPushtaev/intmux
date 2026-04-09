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
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use intmux::{RunOptions, launch_command, launch_shell_command};
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

#[test]
fn shell_command_mode_runs_redirection_in_tmux_shell() -> Result<(), Box<dyn Error>> {
    let harness = TmuxHarness::new()?;
    let workspace = tempfile::tempdir()?;
    let target = workspace.path().join("redirect-output");

    launch_shell_command(
        String::from("echo 123 > redirect-output"),
        workspace.path().to_path_buf(),
        &harness.options,
    )?;

    wait_for_path(&target)?;
    assert_eq!(fs::read_to_string(target)?.trim(), "123");
    Ok(())
}

#[test]
fn reuse_window_reuses_matching_command_and_cwd() -> Result<(), Box<dyn Error>> {
    let harness = TmuxHarness::new()?;
    let workspace = tempfile::tempdir()?;
    let options = harness.options.clone().with_reuse_window();

    launch_command(
        [OsString::from("touch"), OsString::from("sticky-output")],
        workspace.path().to_path_buf(),
        &options,
    )?;
    wait_for_path(&workspace.path().join("sticky-output"))?;

    let first_window_id = harness.window_id(0)?;
    let reuse_key = harness.window_option_value(0, "@intmux.reuse-key-sha256")?;

    launch_command(
        [OsString::from("touch"), OsString::from("sticky-output")],
        workspace.path().to_path_buf(),
        &options,
    )?;

    assert_eq!(harness.window_count()?, 1);
    assert_eq!(harness.window_id(0)?, first_window_id);
    assert_eq!(reuse_key.len(), 64);
    Ok(())
}

#[test]
fn reuse_window_uses_different_windows_for_different_directories() -> Result<(), Box<dyn Error>> {
    let harness = TmuxHarness::new()?;
    let first_workspace = tempfile::tempdir()?;
    let second_workspace = tempfile::tempdir()?;
    let options = harness.options.clone().with_reuse_window();

    launch_command(
        [OsString::from("touch"), OsString::from("shared-name")],
        first_workspace.path().to_path_buf(),
        &options,
    )?;
    launch_command(
        [OsString::from("touch"), OsString::from("shared-name")],
        second_workspace.path().to_path_buf(),
        &options,
    )?;

    wait_for_path(&first_workspace.path().join("shared-name"))?;
    wait_for_path(&second_workspace.path().join("shared-name"))?;
    assert_eq!(harness.window_count()?, 2);
    Ok(())
}

#[test]
fn reuse_window_falls_back_when_matching_window_is_busy() -> Result<(), Box<dyn Error>> {
    let harness = TmuxHarness::new()?;
    let workspace = tempfile::tempdir()?;
    let options = harness.options.clone().with_reuse_window();

    launch_command(
        [OsString::from("touch"), OsString::from("busy-target")],
        workspace.path().to_path_buf(),
        &options,
    )?;
    wait_for_path(&workspace.path().join("busy-target"))?;

    let first_window_id = harness.window_id(0)?;
    harness.send_literal_command(0, "sleep 5")?;
    harness.wait_for_pane_command(0, "sleep")?;

    launch_command(
        [OsString::from("touch"), OsString::from("busy-target")],
        workspace.path().to_path_buf(),
        &options,
    )?;

    assert_eq!(harness.window_count()?, 2);
    assert_eq!(
        harness.window_option_value_by_id(&first_window_id, "@intmux.reuse-key-sha256")?,
        ""
    );
    assert_ne!(harness.window_id(1)?, first_window_id);
    Ok(())
}

#[test]
fn reuse_window_resets_shell_cwd_before_reuse() -> Result<(), Box<dyn Error>> {
    let harness = TmuxHarness::new()?;
    let workspace = tempfile::tempdir()?;
    let options = harness.options.clone().with_reuse_window();
    let file_name = format!("sticky-relative-{}", unique_socket_name()?);
    let target = workspace.path().join(&file_name);
    let tmp_target = Path::new("/tmp").join(&file_name);
    let _ignored = fs::remove_file(&tmp_target);

    launch_command(
        [OsString::from("touch"), OsString::from(&file_name)],
        workspace.path().to_path_buf(),
        &options,
    )?;
    wait_for_path(&target)?;
    fs::remove_file(&target)?;

    harness.send_literal_command(0, "cd /tmp")?;
    launch_command(
        [OsString::from("touch"), OsString::from(&file_name)],
        workspace.path().to_path_buf(),
        &options,
    )?;

    wait_for_path(&target)?;
    assert_eq!(harness.window_count()?, 1);
    assert!(!tmp_target.exists());
    Ok(())
}

#[test]
fn custom_session_name_with_spaces_creates_and_reuses_named_session() -> Result<(), Box<dyn Error>>
{
    let harness = TmuxHarness::new_with_session("team space")?;
    let workspace = tempfile::tempdir()?;
    let options = harness.options.clone().with_reuse_window();

    launch_command(
        [
            OsString::from("touch"),
            OsString::from("named-session-output"),
        ],
        workspace.path().to_path_buf(),
        &options,
    )?;
    wait_for_path(&workspace.path().join("named-session-output"))?;

    launch_command(
        [
            OsString::from("touch"),
            OsString::from("named-session-output"),
        ],
        workspace.path().to_path_buf(),
        &options,
    )?;

    assert_eq!(harness.session_name()?, "team space");
    assert_eq!(harness.window_count()?, 1);
    Ok(())
}

struct TmuxHarness {
    options: RunOptions,
    session_name: String,
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
            session_name: String::from("intmux"),
            socket_name,
            _workspace: workspace,
        })
    }

    fn new_with_session(session_name: &str) -> Result<Self, Box<dyn Error>> {
        let socket_name = unique_socket_name()?;
        let workspace = tempfile::tempdir()?;
        let options = RunOptions::with_socket_name(socket_name.clone())?
            .with_session_name(String::from(session_name))?;

        Ok(Self {
            options,
            session_name: String::from(session_name),
            socket_name,
            _workspace: workspace,
        })
    }

    fn session_name(&self) -> Result<String, Box<dyn Error>> {
        self.tmux_stdout(["list-sessions", "-F", "#{session_name}"])
    }

    fn window_count(&self) -> Result<usize, Box<dyn Error>> {
        let raw = self.tmux_stdout([
            "list-windows",
            "-t",
            self.session_name.as_str(),
            "-F",
            "#{window_id}",
        ])?;
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

    fn window_option_value(&self, index: usize, option: &str) -> Result<String, Box<dyn Error>> {
        let window_id = self.window_id(index)?;
        self.window_option_value_by_id(&window_id, option)
    }

    fn window_option_value_by_id(
        &self,
        window_id: &str,
        option: &str,
    ) -> Result<String, Box<dyn Error>> {
        self.tmux_stdout(["show-options", "-qwv", "-t", window_id, option])
    }

    fn pane_dead(&self, index: usize) -> Result<String, Box<dyn Error>> {
        let pane_id = self.pane_id(index)?;
        self.tmux_stdout(["display-message", "-p", "-t", &pane_id, "#{pane_dead}"])
    }

    fn pane_current_command(&self, index: usize) -> Result<String, Box<dyn Error>> {
        let pane_id = self.pane_id(index)?;
        self.tmux_stdout([
            "display-message",
            "-p",
            "-t",
            &pane_id,
            "#{pane_current_command}",
        ])
    }

    fn send_literal_command(&self, index: usize, command: &str) -> Result<(), Box<dyn Error>> {
        let pane_id = self.pane_id(index)?;
        self.tmux_success(["send-keys", "-t", &pane_id, "-l", command])?;
        self.tmux_success(["send-keys", "-t", &pane_id, "C-m"])?;
        Ok(())
    }

    fn wait_for_pane_command(&self, index: usize, command: &str) -> Result<(), Box<dyn Error>> {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if self.pane_current_command(index)? == command {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(format!("timed out waiting for pane command {command}").into())
    }

    fn window_id(&self, index: usize) -> Result<String, Box<dyn Error>> {
        let raw = self.tmux_stdout([
            "list-windows",
            "-t",
            self.session_name.as_str(),
            "-F",
            "#{window_id}",
        ])?;
        let ids: Vec<&str> = raw.lines().collect();
        ids.get(index)
            .map(|id| (*id).to_owned())
            .ok_or_else(|| format!("missing window at index {index}").into())
    }

    fn pane_id(&self, index: usize) -> Result<String, Box<dyn Error>> {
        let raw = self.tmux_stdout([
            "list-panes",
            "-t",
            self.session_name.as_str(),
            "-F",
            "#{pane_id}",
        ])?;
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
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
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
