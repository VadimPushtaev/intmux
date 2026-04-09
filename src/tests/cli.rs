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
fn compute_reuse_key_changes_with_working_directory() {
    let argv = vec![OsString::from("touch"), OsString::from("target-file")];
    let first = compute_reuse_key(PathBuf::from("/one").as_path(), &argv);
    let second = compute_reuse_key(PathBuf::from("/two").as_path(), &argv);
    assert_ne!(first, second);
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
    assert_eq!(spec.window_name(), "printf");
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
