//! #0 show-install
use crate::command::CmdArg;

pub(crate) fn run(carg: CmdArg) -> Result<(), anyhow::Error>
{
	// Setup dirs
	let rtdirs = crate::core::RtDirs::init(&carg.config.basedir(),
			&carg.config.workdir())?;

	// Split upt
	let CmdArg { clargs, config: _, version } = carg;

	// Extract our own args
	let args = match clargs.command {
		crate::command::FrCmds::ShowInstall(a) => a,
		_ => unreachable!("I'm a show-install, why does it think I'm not??"),
	};

	// Load up the state and see what's in the manifest
	let state = match rtdirs.state_load_raw()? {
		Some(s) => s,
		None => {
			println!("No state to load; no fetch/upgrade has been run?");
			// XXX Should this be an Err() for a nonzero exit?
			return Ok(());
		},
	};
	let manifest = match state.manifest {
		Some(m) => m,
		None => {
			println!("No install pending.");
			return Ok(());
		},
	};

	// Simplify verbosity checking
	let isverb = |t: &str| {
		// Special case: 'any' means anything is given
		if t == "any" { return args.verbose.len() > 0; }

		// If All is in the list, it's true
		use crate::command::ShowInstallType as SIT;
		if args.verbose.contains(&SIT::All) { return true; }

		// Anything else should be turnable into a SIT.
		let sit = t.try_into().expect("Should be valid, bad programmer");
		if args.verbose.contains(&sit) { return true; }

		// Else, nyet
		false
	};

	// Say what we want to do
	let upvers = manifest.version();
	let mt = manifest.mtype();
	let cmdname = crate::util::cmdname();
	if isverb("any")
	{
		println!("Changes for pending {mt} from {version} to {upvers}:");
	}
	else
	{
		println!("Summary of pending {mt} from {version} to {upvers}");
		println!("    (use `{cmdname} show-install -v` to show full details)");
	}


	// Added/removed/updated files
	let sum = manifest.change_summary();
	let steps = [
		("add",    sum.added),
		("remove", sum.removed),
		("update", sum.updated),
	];
	for (act, files) in steps
	{
		let num = files.len();
		match num
		{
			0 => {
				if isverb(act)
				{ println!("\n No files to {act}"); }
				else
				{ println!(" No files to {act}"); }
				continue;
			},
			_ => {
				if isverb(act)
				{
					println!("\n {num} files to {act}:");
					for f in files { let f = f.display(); println!("  {f}"); }
				}
				else
				{
					println!(" {num} files to {act}.");
				}
			},
		}
	}


	// Changed types
	let tchanges = manifest.type_changes();
	let nch = tchanges.len();
	if nch > 0
	{
		if isverb("change") { println!(""); }
		println!(" {nch} paths with changed type.");
		if isverb("change")
		{
			use itertools::Itertools as _; // .sorted()
			for p in tchanges.keys().sorted()
			{
				let ch = tchanges.get(p).unwrap();
				println!("  {} changed from {} to {}", p.display(),
				ch.old.ftype(), ch.new.ftype());
			}
		}
	}


	// Merged files
	use crate::state::Manifest;
	if let Manifest::Upgrade(mup) = &manifest
	{
		use crate::util::plural;

		if isverb("merge")
		{
			println!("");
			let clean = &mup.merge_clean;
			if clean.len() > 0
			{
				let num = clean.len();
				println!(" {num} merged file{}:", plural(num));
				for f in clean.keys() { println!("  {}", f.display()); }
				println!("    `{cmdname} show-merges` for details");
			}
			else
			{
				println!(" No merged files.");
			}

			let cfs = &mup.merge_conflict;
			if cfs.len() > 0
			{
				let num = cfs.len();
				println!(" {num} merge conflict{}:", plural(num));
				for f in cfs.keys() { println!("  {}", f.display()); }
				println!("    `{cmdname} resolve-merges` to resolve");
			}
			else
			{
				println!(" No merge conflicts.");
			}
		}
		else
		{
			let num = mup.num_clean();
			if num > 0
			{
				println!(" {num} clean merge{}; see `{cmdname} show-merges` \
						for details", plural(num));
			}
			else
			{
				println!(" No merged files.");
			}

			let num = mup.num_conflict();
			if num > 0
			{
				println!(" {num} outstanding conflicted merge{}; run \
						`{cmdname} resolve-merges` to resolve.", plural(num));
			}
			else
			{
				println!(" No conflicts to resolve.");
			}
		}
	}


	// Now say something about the overall state.
	let ststr = manifest.state();
	println!("\n{ststr}.");

	Ok(())
}
