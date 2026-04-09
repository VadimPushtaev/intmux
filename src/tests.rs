use super::*;
use std::collections::VecDeque;

#[derive(Debug)]
struct FakeRunner {
    outputs: VecDeque<io::Result<ProcessOutput>>,
    calls: Vec<Vec<OsString>>,
}

impl FakeRunner {
    fn new(outputs: Vec<io::Result<ProcessOutput>>) -> Self {
        Self {
            outputs: VecDeque::from(outputs),
            calls: Vec::new(),
        }
    }
}

impl TmuxRunner for FakeRunner {
    fn run(&mut self, args: &[OsString]) -> io::Result<ProcessOutput> {
        self.calls.push(args.to_vec());
        self.outputs
            .pop_front()
            .unwrap_or_else(|| panic!("missing fake output for call {args:?}"))
    }
}

fn success(stdout: &str) -> ProcessOutput {
    ProcessOutput {
        status_code: Some(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

fn failure(code: i32, stderr: &str) -> ProcessOutput {
    ProcessOutput {
        status_code: Some(code),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn cli_accepts_command_without_double_dash() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::try_parse_from(["intmux", "ls", "/tmp"])?;
    assert_eq!(
        cli.command,
        vec![OsString::from("ls"), OsString::from("/tmp")]
    );
    Ok(())
}

#[test]
fn cli_accepts_trailing_command_after_double_dash() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::try_parse_from(["intmux", "--", "ls", "/tmp"])?;
    assert_eq!(
        cli.command,
        vec![OsString::from("ls"), OsString::from("/tmp")]
    );
    Ok(())
}

#[test]
fn cli_treats_hyphenated_values_after_command_as_command_args()
-> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::try_parse_from(["intmux", "printf", "--version"])?;
    assert_eq!(
        cli.command,
        vec![OsString::from("printf"), OsString::from("--version")]
    );
    Ok(())
}

#[test]
fn run_options_validate_socket_names() {
    assert_eq!(
        RunOptions::with_socket_name(String::new()),
        Err(ConfigError::EmptySocketName)
    );
    assert_eq!(
        RunOptions::with_socket_name(String::from("bad/name")),
        Err(ConfigError::SocketNameContainsSeparator)
    );
}

#[test]
fn command_spec_rejects_empty_commands() {
    let result = CommandSpec::new(Vec::<OsString>::new(), PathBuf::from("/tmp"));
    assert!(matches!(result, Err(IntmuxError::InvalidCommand(_))));
}

#[test]
fn command_spec_derives_window_name_from_basename() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CommandSpec::new(
        [OsString::from("/usr/bin/printf"), OsString::from("ok")],
        PathBuf::from("/tmp"),
    )?;
    assert_eq!(spec.window_name, "printf");
    Ok(())
}

#[test]
fn parse_create_target_rejects_bad_output() {
    let result = parse_create_target("oops", "create tmux window");
    assert!(matches!(
        result,
        Err(IntmuxError::UnexpectedTmuxOutput { .. })
    ));
}

#[test]
fn shell_quote_handles_spaces_and_single_quotes() {
    assert_eq!(shell_quote(OsStr::new("plain-text")), "plain-text");
    assert_eq!(shell_quote(OsStr::new("two words")), "'two words'");
    assert_eq!(shell_quote(OsStr::new("it's")), "'it'\\''s'");
}

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
