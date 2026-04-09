use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;

use super::*;

mod cli;
mod tmux_behavior;
mod tmux_reuse;

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
