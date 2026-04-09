use std::ffi::OsString;
use std::path::PathBuf;

use super::*;

#[test]
fn missing_session_creates_session_then_sends_command() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CommandSpec::new(
        [OsString::from("ls"), OsString::from("/tmp")],
        PathBuf::from("/work"),
    )?;
    let options = RunOptions::with_socket_name(String::from("test-socket"))?;
    let mut runner = FakeRunner::new(vec![
        Ok(failure(1, "can't find session: intmux")),
        Ok(success("@1\t%2\n")),
        Ok(success("")),
        Ok(success("0\tbash")),
        Ok(success("ls /tmp")),
        Ok(success("")),
    ]);

    launch_with_runner(&mut runner, &spec, &options)?;

    assert_eq!(
        runner.calls,
        vec![
            vec![
                OsString::from("-L"),
                OsString::from("test-socket"),
                OsString::from("has-session"),
                OsString::from("-t"),
                OsString::from("intmux"),
            ],
            vec![
                OsString::from("-L"),
                OsString::from("test-socket"),
                OsString::from("new-session"),
                OsString::from("-d"),
                OsString::from("-P"),
                OsString::from("-F"),
                OsString::from("#{window_id}\t#{pane_id}"),
                OsString::from("-s"),
                OsString::from("intmux"),
                OsString::from("-n"),
                OsString::from("ls"),
                OsString::from("-c"),
                OsString::from("/work"),
            ],
            vec![
                OsString::from("-L"),
                OsString::from("test-socket"),
                OsString::from("set-window-option"),
                OsString::from("-t"),
                OsString::from("@1"),
                OsString::from("automatic-rename"),
                OsString::from("off"),
            ],
            vec![
                OsString::from("-L"),
                OsString::from("test-socket"),
                OsString::from("display-message"),
                OsString::from("-p"),
                OsString::from("-t"),
                OsString::from("%2"),
                OsString::from("#{pane_dead}\t#{pane_current_command}"),
            ],
            vec![
                OsString::from("-L"),
                OsString::from("test-socket"),
                OsString::from("send-keys"),
                OsString::from("-t"),
                OsString::from("%2"),
                OsString::from("-l"),
                OsString::from("ls /tmp"),
            ],
            vec![
                OsString::from("-L"),
                OsString::from("test-socket"),
                OsString::from("send-keys"),
                OsString::from("-t"),
                OsString::from("%2"),
                OsString::from("C-m"),
            ],
        ]
    );
    Ok(())
}

#[test]
fn existing_session_adds_window() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CommandSpec::new(
        [OsString::from("touch"), OsString::from("file with spaces")],
        PathBuf::from("/work"),
    )?;
    let options = RunOptions::default();
    let mut runner = FakeRunner::new(vec![
        Ok(success("")),
        Ok(success("@4\t%9\n")),
        Ok(success("")),
        Ok(success("0\tbash")),
        Ok(success("touch 'file with spaces'")),
        Ok(success("")),
    ]);

    launch_with_runner(&mut runner, &spec, &options)?;

    assert_eq!(runner.calls[1][0], OsString::from("new-window"));
    assert_eq!(runner.calls[4][0], OsString::from("send-keys"));
    assert_eq!(runner.calls[4][3], OsString::from("-l"));
    assert_eq!(
        runner.calls[4][4],
        OsString::from("touch 'file with spaces'")
    );
    Ok(())
}

#[test]
fn wait_for_live_pane_retries_until_shell_is_ready() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CommandSpec::new([OsString::from("ls")], PathBuf::from("/work"))?;
    let options = RunOptions::default();
    let mut runner = FakeRunner::new(vec![
        Ok(failure(1, "can't find session: intmux")),
        Ok(success("@1\t%2\n")),
        Ok(success("")),
        Ok(success("1\t")),
        Ok(success("0\tbash")),
        Ok(success("ls")),
        Ok(success("")),
    ]);

    launch_with_runner(&mut runner, &spec, &options)?;

    assert_eq!(
        runner
            .calls
            .iter()
            .filter(|call| call
                .iter()
                .any(|arg| arg == "#{pane_dead}\t#{pane_current_command}"))
            .count(),
        2
    );
    Ok(())
}
