//! #0 show-merges
use crate::command::CmdArg;

pub(crate) fn run(carg: CmdArg) -> Result<(), anyhow::Error>
{
	// Setup dirs
	let rtdirs = crate::core::RtDirs::init(&carg.config.basedir(),
			&carg.config.workdir())?;

	// Split up
	let CmdArg { clargs, config: _, version } = carg;

	// Extract our own args
	let args = match clargs.command {
		crate::command::FrCmds::ShowMerges(a) => a,
		_ => unreachable!("I'm a show-merges, why does it think I'm not??"),
	};

	// Load up the state and see what's in the manifest
	let state = match rtdirs.state_load_raw()? {
		Some(s) => s,
		None => {
			println!("No state to load; no pending upgrade?");
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

	// If this isn't an upgrade, there ain't no merges to show...
	if manifest.mtype() != "upgrade"
	{
		println!("No pending upgrade, no merges to show.");
		return Ok(());
	}

	// Summaryize
	let cmdname = crate::util::cmdname();
	let upvers = manifest.version();
	println!("Pending upgrade from {version} to {upvers}.");

	use crate::state::Manifest;
	let mup = match manifest {
		Manifest::Upgrade(m) => m,
		_ => unreachable!("We already know it's an upgrade"),
	};


	// Util
	// XXX Very simliar to over in upgrade.rs; if we start needing this a
	// third time, we should probably generalize...
	let mf_data = |hb: &crate::util::hash::Sha256HashBuf| -> Result<_, _> {
		let gzf = rtdirs.hashfile(hb);
		crate::util::compress::decompress_to_vec(&gzf)
	};


	// Detailyize
	use crate::util::plural;
	use itertools::Itertools as _; // .sorted()
	let clean = mup.merge_clean;
	let num = clean.len();

	println!("{num} merged file{}", plural(num));
	if args.upstream { println!("  (diffs against new upstream versions)"); }
	println!("");

	for f in clean.keys().sorted()
	{
		let cd = clean.get(f).unwrap();
		let prev = match args.upstream {
			true  => mf_data(&cd.new),
			false => mf_data(&cd.cur),
		}?;
		let new  = mf_data(&cd.res)?;

		// We assume it's _probably_ valid UTF8, so easy to deal with.
		// If it's not, we do our best, but warn about it.
		let dbytes = crate::core::merge::merge_diff(f, &prev, &new);
		let dstr = String::from_utf8_lossy(&dbytes);

		use std::borrow::Cow::{Owned, Borrowed};
		let lstr = match dstr {
			Owned(_)    => "  (include non UTF-8 chars, lossily converted)",
			Borrowed(_) => "",
		};


		println!("diff {}{lstr}\n{dstr}\n", f.display());
	}


	let ncf = mup.merge_conflict.len();
	if ncf > 0
	{
		println!("{ncf} conflict{} still to be resolved; \
				run `{cmdname} resolve-merges` to deal with them.\n",
				plural(ncf));
	}



	Ok(())
}
