use freebsd_rustdate as FR;
use std::process::ExitCode;

fn main() -> Result<ExitCode, anyhow::Error>
{
	// What are we supposed to do?
	let cmd = FR::command::parse();

	// Doit
	FR::command::run(cmd)
}
