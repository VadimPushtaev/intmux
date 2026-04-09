use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::model::{CommandInput, CommandSpec};

pub(crate) const REUSE_WINDOW_OPTION: &str = "@intmux.reuse-key-sha256";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReuseKey(String);

impl ReuseKey {
    pub(crate) fn from_command_spec(spec: &CommandSpec) -> Self {
        match spec.command_input() {
            CommandInput::Argv(argv) => Self(compute_reuse_key(spec.cwd(), argv)),
            CommandInput::Shell(command_line) => {
                Self(compute_shell_command_reuse_key(spec.cwd(), command_line))
            }
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn compute_reuse_key(cwd: &Path, argv: &[std::ffi::OsString]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"argv");
    update_with_os_bytes(&mut hasher, cwd.as_os_str());
    hasher.update([0_u8]);
    for argument in argv {
        update_with_os_bytes(&mut hasher, argument.as_os_str());
        hasher.update([0_u8]);
    }
    hex_encode(hasher.finalize().as_slice())
}

pub(crate) fn compute_shell_command_reuse_key(cwd: &Path, command_line: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"shell");
    update_with_os_bytes(&mut hasher, cwd.as_os_str());
    hasher.update([0_u8]);
    hasher.update(command_line.as_bytes());
    hasher.update([0_u8]);
    hex_encode(hasher.finalize().as_slice())
}

fn update_with_os_bytes(hasher: &mut Sha256, value: &OsStr) {
    hasher.update(value.as_bytes());
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(hex_nibble(byte >> 4));
        output.push(hex_nibble(byte & 0x0f));
    }
    output
}

fn hex_nibble(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        10..=15 => char::from(b'a' + (value - 10)),
        _ => unreachable!("nibble must be in range 0..=15"),
    }
}
