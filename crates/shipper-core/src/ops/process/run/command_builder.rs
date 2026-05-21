use std::process::{Command, Stdio};

pub(super) fn base_command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    command
}

pub(super) fn streaming_command(program: &str, args: &[&str]) -> Command {
    let mut command = base_command(program, args);
    command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    command
}
