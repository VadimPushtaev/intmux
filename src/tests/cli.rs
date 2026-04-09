use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use super::*;

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
fn cli_accepts_reuse_window_flag() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::try_parse_from(["intmux", "--reuse-window", "ls", "/tmp"])?;
    assert!(cli.reuse_window);
    assert_eq!(
        cli.command,
        vec![OsString::from("ls"), OsString::from("/tmp")]
    );
    Ok(())
}

#[test]
fn cli_accepts_shell_command_flag() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::try_parse_from(["intmux", "-c", "echo 123 > /tmp/file"])?;
    assert_eq!(cli.shell_command.as_deref(), Some("echo 123 > /tmp/file"));
    assert!(cli.command.is_empty());
    Ok(())
}

#[test]
fn cli_rejects_shell_command_mixed_with_argv_command() {
    let result = Cli::try_parse_from(["intmux", "-c", "echo 123", "ls"]);
    assert!(result.is_err());
}

#[test]
fn cli_accepts_custom_session_flag() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::try_parse_from(["intmux", "--session", "team space", "ls", "/tmp"])?;
    assert_eq!(
        cli.session.as_ref().map(SessionName::as_str),
        Some("team space")
    );
    assert_eq!(
        cli.command,
        vec![OsString::from("ls"), OsString::from("/tmp")]
    );
    Ok(())
}

#[test]
fn cli_rejects_invalid_session_names() {
    let result = Cli::try_parse_from(["intmux", "--session", "bad:name", "ls"]);
    assert!(result.is_err());
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
fn run_options_validate_session_names() {
    assert_eq!(
        RunOptions::new().with_session_name(String::new()),
        Err(ConfigError::EmptySessionName)
    );
    assert_eq!(
        RunOptions::new().with_session_name(String::from("bad:name")),
        Err(ConfigError::SessionNameContainsColon)
    );
    assert_eq!(
        RunOptions::new()
            .with_session_name(String::from("team space"))
            .map(|options| String::from(options.session_name())),
        Ok(String::from("team space"))
    );
}

#[test]
fn merge_run_options_prefers_cli_session_name() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::try_parse_from(["intmux", "--session", "cli-name", "ls"])?;
    let options = RunOptions::new().with_session_name(String::from("base-name"))?;
    let merged = merge_run_options(&cli, &options);

    assert_eq!(merged.session_name(), "cli-name");
    Ok(())
}

#[test]
fn compute_reuse_key_changes_with_working_directory() {
    let argv = vec![OsString::from("touch"), OsString::from("target-file")];
    let first = compute_reuse_key(PathBuf::from("/one").as_path(), &argv);
    let second = compute_reuse_key(PathBuf::from("/two").as_path(), &argv);
    assert_ne!(first, second);
}

#[test]
fn shell_command_reuse_key_differs_from_argv_reuse_key() {
    let argv = vec![OsString::from("echo"), OsString::from("123")];
    let cwd = PathBuf::from("/one");
    let argv_key = compute_reuse_key(cwd.as_path(), &argv);
    let shell_key = compute_shell_command_reuse_key(cwd.as_path(), "echo 123");

    assert_ne!(argv_key, shell_key);
}

#[test]
fn command_spec_rejects_empty_commands() {
    let result = CommandSpec::new(Vec::<OsString>::new(), PathBuf::from("/tmp"));
    assert!(matches!(result, Err(IntmuxError::InvalidCommand(_))));
}

#[test]
fn shell_command_spec_rejects_empty_commands() {
    let result = CommandSpec::from_shell_command(String::from("   "), PathBuf::from("/tmp"));
    assert!(matches!(result, Err(IntmuxError::InvalidCommand(_))));
}

#[test]
fn command_spec_derives_window_name_from_basename() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CommandSpec::new(
        [OsString::from("/usr/bin/printf"), OsString::from("ok")],
        PathBuf::from("/tmp"),
    )?;
    assert_eq!(spec.window_name(), "printf");
    Ok(())
}

#[test]
fn shell_command_spec_uses_literal_command_line() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CommandSpec::from_shell_command(
        String::from("echo 123 > relative-output"),
        PathBuf::from("/tmp"),
    )?;
    assert_eq!(spec.window_name(), "echo");
    assert_eq!(spec.rendered_command_line(), "echo 123 > relative-output");
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
