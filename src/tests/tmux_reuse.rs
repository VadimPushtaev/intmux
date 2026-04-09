use std::ffi::OsString;
use std::path::PathBuf;

use super::*;

#[test]
fn reuse_window_uses_matching_live_shell() -> Result<(), Box<dyn std::error::Error>> {
    let argv = [OsString::from("touch"), OsString::from("file with spaces")];
    let spec = CommandSpec::new(argv.iter().cloned(), PathBuf::from("/work"))?;
    let reuse_key = compute_reuse_key(spec.cwd(), &argv);
    let options = RunOptions::with_socket_name(String::from("test-socket"))?.with_reuse_window();
    let filter = format!("#{{==:#{{@intmux.reuse-key-sha256}},{reuse_key}}}");
    let mut runner = FakeRunner::new(vec![
        Ok(success("")),
        Ok(success("@7\t4\t1\t%10\t0\tbash\n@4\t1\t1\t%9\t0\tbash\n")),
        Ok(success("/bin/bash")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
    ]);

    launch_with_runner(&mut runner, &spec, &options)?;

    assert_eq!(runner.calls, expected_reuse_calls(&filter, &reuse_key));
    Ok(())
}

#[test]
fn reuse_window_falls_back_to_new_window_for_busy_match() -> Result<(), Box<dyn std::error::Error>>
{
    let argv = [OsString::from("ls"), OsString::from("/tmp")];
    let spec = CommandSpec::new(argv.iter().cloned(), PathBuf::from("/work"))?;
    let reuse_key = compute_reuse_key(spec.cwd(), &argv);
    let options = RunOptions::with_socket_name(String::from("test-socket"))?.with_reuse_window();
    let filter = format!("#{{==:#{{@intmux.reuse-key-sha256}},{reuse_key}}}");
    let mut runner = FakeRunner::new(vec![
        Ok(success("")),
        Ok(success("@4\t1\t1\t%9\t0\tsleep")),
        Ok(success("/bin/bash")),
        Ok(success("")),
        Ok(success("@8\t%11\n")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("0\tbash")),
        Ok(success("ls /tmp")),
        Ok(success("")),
    ]);

    launch_with_runner(&mut runner, &spec, &options)?;

    assert_eq!(runner.calls[1][2], OsString::from("list-windows"));
    assert_eq!(runner.calls[1][6], OsString::from(filter));
    assert_eq!(runner.calls[3][5], OsString::from("@4"));
    assert_eq!(runner.calls[4][2], OsString::from("new-window"));
    assert_eq!(
        runner.calls[6][5],
        OsString::from("@intmux.reuse-key-sha256")
    );
    assert_eq!(runner.calls[8][6], OsString::from("ls /tmp"));
    Ok(())
}

#[test]
fn reuse_window_uses_custom_session_for_lookup() -> Result<(), Box<dyn std::error::Error>> {
    let argv = [OsString::from("ls")];
    let spec = CommandSpec::new(argv.iter().cloned(), PathBuf::from("/work"))?;
    let options = RunOptions::with_socket_name(String::from("test-socket"))?
        .with_session_name(String::from("team space"))?
        .with_reuse_window();
    let reuse_key = compute_reuse_key(spec.cwd(), &argv);
    let filter = format!("#{{==:#{{@intmux.reuse-key-sha256}},{reuse_key}}}");
    let mut runner = FakeRunner::new(vec![
        Ok(success("")),
        Ok(success("@4\t1\t1\t%9\t0\tbash")),
        Ok(success("/bin/bash")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
        Ok(success("")),
    ]);

    launch_with_runner(&mut runner, &spec, &options)?;

    assert_eq!(runner.calls[0][4], OsString::from("team space"));
    assert_eq!(runner.calls[1][4], OsString::from("team space"));
    assert_eq!(runner.calls[1][6], OsString::from(filter));
    Ok(())
}

fn expected_reuse_calls(filter: &str, reuse_key: &str) -> Vec<Vec<OsString>> {
    vec![
        tmux_call(["has-session", "-t", "intmux"]),
        vec![
            OsString::from("-L"),
            OsString::from("test-socket"),
            OsString::from("list-windows"),
            OsString::from("-t"),
            OsString::from("intmux"),
            OsString::from("-f"),
            OsString::from(filter),
            OsString::from("-F"),
            OsString::from(
                "#{window_id}\t#{window_index}\t#{window_panes}\t#{pane_id}\t#{pane_dead}\t#{pane_current_command}",
            ),
        ],
        tmux_call(["show-options", "-v", "-g", "default-shell"]),
        tmux_call([
            "set-window-option",
            "-u",
            "-t",
            "@7",
            "@intmux.reuse-key-sha256",
        ]),
        tmux_call(["set-window-option", "-t", "@4", "automatic-rename", "off"]),
        vec![
            OsString::from("-L"),
            OsString::from("test-socket"),
            OsString::from("set-window-option"),
            OsString::from("-t"),
            OsString::from("@4"),
            OsString::from("@intmux.reuse-key-sha256"),
            OsString::from(reuse_key),
        ],
        tmux_call(["send-keys", "-t", "%9", "C-c"]),
        tmux_call(["send-keys", "-t", "%9", "-l", "cd -- /work"]),
        tmux_call(["send-keys", "-t", "%9", "C-m"]),
        tmux_call(["send-keys", "-t", "%9", "-l", "touch 'file with spaces'"]),
        tmux_call(["send-keys", "-t", "%9", "C-m"]),
    ]
}

fn tmux_call<const N: usize>(args: [&str; N]) -> Vec<OsString> {
    let mut call = vec![OsString::from("-L"), OsString::from("test-socket")];
    call.extend(args.into_iter().map(OsString::from));
    call
}
