# intmux

`intmux` launches commands into a tmux session without attaching to it or stealing focus.

It is useful when you want a persistent tmux window for each launched command, especially if you want to inspect what happened after the command finishes.

## Why

`intmux` gives you a simple way to send work into tmux from scripts, shells, or other tools:

- create or reuse a tmux session
- open a fresh window, or reuse a sticky one when requested
- run the command inside an interactive shell
- leave the shell usable after the command finishes

This is handy for long-running jobs, background work, and debugging command execution without cluttering your current terminal.

## Install

From the repository root:

```bash
cargo install --path .
```

That installs `intmux` into `~/.cargo/bin/intmux`.

## Basic Usage

Run a normal argv-style command:

```bash
intmux ls /tmp
```

Use a specific tmux session:

```bash
intmux --session work ls /tmp
```

Reuse the same tmux window for the same command line and working directory:

```bash
intmux --reuse-window ls /tmp
```

Pass a raw shell command line when you need shell syntax such as redirection or pipes:

```bash
intmux -c 'echo 123 > /tmp/file'
intmux -c 'tail -f /var/log/syslog | grep ssh'
```

If you want shell-command mode to reuse the same window too, combine it with `--reuse-window`:

```bash
intmux --reuse-window -c 'echo 123 > /tmp/file'
```

## How It Behaves

By default `intmux` targets the tmux session named `intmux`.

When the session does not exist, `intmux` creates it. When it already exists, `intmux` creates a new detached window in that session. The command is typed into a live shell inside that window, so the pane remains interactive after the command completes.

`--reuse-window` changes only the window-selection behavior. It computes a checksum from the current working directory plus the command payload and tries to find an existing matching window in the same tmux session. If it finds a suitable live shell window, it reuses it; otherwise it creates a new one.

## Following AI Agents

`intmux` can be useful if you want to follow everything AI agents do in tmux instead of letting commands disappear into a hidden subprocess.

The important limitation is that the agent must actually invoke commands through `intmux`. In practice that means asking the agent to prefix every command with `intmux`, or with `intmux -c` when shell syntax is needed.

Examples:

```bash
intmux cargo test
intmux --reuse-window cargo check
intmux -c 'pytest -q > /tmp/pytest.log 2>&1'
```

With that setup, you can keep tmux open in another terminal and watch the windows appear as the agent works.

## CLI Summary

```text
Usage: intmux [OPTIONS] [COMMAND [ARGS]...]...

Options:
      --session <NAME>           Target a specific tmux session instead of the default `intmux`
  -c, --shell-command <COMMAND>  Run a shell command line inside tmux without local shell parsing
      --reuse-window             Reuse a previously tagged tmux window for the same command and working directory
  -h, --help                     Print help
  -V, --version                  Print version
```

## Development

Run the full local gate:

```bash
make pre-commit
```
