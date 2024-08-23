//! #0 clean
use crate::command::CmdArg;

pub(crate) fn run(carg: CmdArg) -> Result<(), anyhow::Error>
{
	// Setup dirs
	let rtdirs = crate::core::RtDirs::init(&carg.config.basedir(),
			&carg.config.workdir())?;

	// Split up
	let CmdArg { clargs, config: _, version: _ } = carg;

	// Extract our own args
	let args = match clargs.command {
		crate::command::FrCmds::Clean(a) => a,
		_ => unreachable!("I'm a clean, why does it think I'm not??"),
	};

	// Maybe we have state
	let state = rtdirs.state_load_raw()?;


	// Currently everything clean might do is behind --options, so
	// specifying none of them means we do nothing...  easiest way to
	// warn about that is a little tracking.
	let mut did = false;


	// Clean up pending state; i.e., forget about a fetch/upgrade we did.
	if args.pending
	{
		did = true;  // didit

		// Clear out the manifest if there is one
		if let Some(mut st) = state
		{
			match st.manifest
			{
				None => println!("No pending updates to clear."),
				Some(m) => {
					let mt = m.mtype();
					st.manifest = None;
					rtdirs.state_save(&st)?;
					println!("Pending {mt} cleared.");
				},
			}
		}
		else
		{
			println!("No current state to clear pending from.");
		}
	}



	// If we didn't [potentially] do something, that's presumably not
	// what the user really wanted, so mention it...
	if !did
	{
		anyhow::bail!("Nothing requested to be done; did you miss an --arg?");
	}



	Ok(())
}
