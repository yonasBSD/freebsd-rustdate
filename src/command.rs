//! General f-u command handling.  This is sorta the central dispatch for
//! everything that goes on.

/// Command-line parsing and handling
mod line;
pub(crate) use line::FrArgs;
pub(crate) use line::FrCmds;
pub(crate) use line::{ShowInstallType, CheckSysIgnore};
pub(crate) use line::FrCmdInstall;
pub use line::parse;



// Handle exiting with a code in special cases
use std::process::ExitCode;

#[derive(Debug)]
enum MyExit
{
	Ok,
	Code(u8),
}

impl From<()> for MyExit { fn from(_x: ()) -> Self { Self::Ok } }
impl From<u8> for MyExit { fn from(c: u8)  -> Self { Self::Code(c) } }

impl From<MyExit> for ExitCode
{
	fn from(my: MyExit) -> Self
	{
		use MyExit as M;
		match my {
			M::Ok      => Self::SUCCESS,
			M::Code(c) => c.into(),
		}
	}
}


/// Pass a bunch of info to the individual command runners as a block
#[derive(Debug)]
pub(crate) struct CmdArg
{
	/// The command-line args
	pub(crate) clargs: FrArgs,

	/// The working config
	pub(crate) config: crate::config::Config,

	/// The current system version
	pub(crate) version: crate::info::Version,
}


/// Dispatch a command
pub fn run(clargs: FrArgs) -> Result<ExitCode, anyhow::Error>
{
	use crate::*;

	// Load up config
	let config = config::load_config_file(&clargs.config, &clargs)?;

	// Any early initalization
	init(&clargs)?;

	// We'll want version info usually
	let version = match &clargs.fakeversion {
		Some(x) => crate::info::version::fake(x)?,
		None => crate::info::version::get(config.basedir())?,
	};

	let carg = CmdArg { clargs, config, version };

	use line::FrCmds as FC;
	let myex: MyExit = match carg.clargs.command {
		// Action
		FC::Fetch{..}   => cmd::fetch::run(carg)?.into(),
		FC::Cron{..}    => cmd::cron::run(carg)?.into(),
		FC::Upgrade{..} => cmd::upgrade::run(carg)?.into(),
		FC::Install{..} => cmd::install::run(carg)?.into(),
		FC::Extract{..} => cmd::extract::run(carg)?.into(),
		FC::CheckSys{..} => cmd::check_sys::run(carg)?.into(),
		FC::CheckFetch{..} => cmd::check_fetch::run(carg)?.into(),

		// Show
		FC::ShowInstall{..} => cmd::show_install::run(carg)?.into(),
		FC::ShowMerges{..}  => cmd::show_merges::run(carg)?.into(),

		// Misc
		FC::Clean{..} => cmd::clean::run(carg)?.into(),
		FC::ResolveMerges{..} => cmd::resolve_merges::run(carg)?.into(),

		// Dev
		FC::DumpMetadata{..} => cmd::dump_metadata::run(carg)?.into(),

		// Fake
		#[cfg(test)]
		FC::Dummy => unreachable!("Not a real thing"),

		// I'll get to it, I swear...
		// x => todo!("Command {} not yet implemented", x),
	};
	Ok(myex.into())
}


/// Do any initalization we care about
pub fn init(_clargs: &FrArgs) -> Result<(), anyhow::Error>
{
	// Init cached euid; we don't change perms during the run, so...
	crate::util::set_euid();

	// Setup u/gid comparison flag
	crate::metadata::init_ugid_cmp();

	Ok(())
}
