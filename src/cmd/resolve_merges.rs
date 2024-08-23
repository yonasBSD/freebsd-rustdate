//! #0 resolve-merges
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
		crate::command::FrCmds::ResolveMerges(a) => a,
		_ => unreachable!("I'm a resolve-merges, why does it think I'm not??"),
	};

	// Load up the state and see what's in the manifest
	let mut state = match rtdirs.state_load_raw()? {
		Some(s) => s,
		None => {
			println!("No state to load; no pending upgrade?");
			// XXX Should this be an Err() for a nonzero exit?
			return Ok(());
		},
	};
	let manifest = match &mut state.manifest {
		Some(m) => m,
		None => {
			println!("No install pending.");
			return Ok(());
		},
	};

	// If this isn't an upgrade, there can't be anything to resolve...
	if manifest.mtype() != "upgrade"
	{
		println!("No pending upgrade, can't be any merges.");
		return Ok(());
	}

	// Summaryize
	let upvers = manifest.version();
	println!("Pending upgrade from {version} to {upvers}");

	use crate::state::Manifest;
	let mup = match manifest {
		Manifest::Upgrade(m) => m,
		_ => unreachable!("We already know it's an upgrade"),
	};


	// Special case; --exit
	if args.exit
	{
		let nc = mup.merge_conflict.len();
		match nc
		{
			0 => { println!("No conflicts"); return Ok(()); },
			_ => anyhow::bail!("{nc} conflicts needing resolution"),
		}
	}



	// Detailyize
	use crate::util::plural;
	let cmdname = crate::util::cmdname();
	let conflicts = &mut mup.merge_conflict;
	let nconfls = conflicts.len();

	if nconfls == 0
	{
		println!("No conflicts to resolve.  You may review the merge results \
			using\n{cmdname} show-merges\nor install the upgrade with\n\
			{cmdname} install");
		return Ok(());
	}
	println!("{nconfls} conflicted merge{}", plural(nconfls));


	// Now resolve the various files
	//
	// This simple case will be spawning off an $EDITOR on each one for
	// manual fiddling.  So, do a quick tty check.
	use std::io;
	use io::IsTerminal as _;
	if !io::stdin().is_terminal()
	{
		anyhow::bail!("stdin doesn't appear to be a tty, bailing...");
	}

	// Check up on $EDITOR config.
	// edit crate seems to not _quite_ be exactly what we'd want, but
	// probably good enough without either handwriting something way too
	// weak, or handwriting way too much...
	let editor = edit::get_editor()?;
	println!("Using `{}` as editor", editor.display());


	// Now, at a time.
	let mrgdir = rtdirs.tmp().join("merge");
	let mut cfnum = 0;
	let mut fixed = 0;
	let mut inline = String::new();
	let cfmarker = regex_lite::Regex
			::new(r"(?m)^(?:(?:<{7}|>{7}|\|{7}) .*|========)$")?;

	use itertools::Itertools as _; // .sorted()
	let cfkeys: Vec<_> = conflicts.keys().sorted().cloned().collect();
	'cfiles: for f in &cfkeys
	{
		let cd = conflicts.get(f).unwrap();

		// Pull out the resulting conflicted file
		let mrgf = {
			let mf = crate::util::path_join(&mrgdir, f);
			let mfd = mf.parent().expect("This can't be tiny...");
			if !mfd.is_dir() { std::fs::create_dir_all(mfd)?; }
			mf
		};
		rtdirs.decompress_hash_file(&cd.res, &mrgf)?;

		// Say it
		cfnum += 1;
		println!("\n[{cfnum}/{nconfls}] Conflicts found in {}.  Press 'e' \
				to spawn off editor and resolve, or 's' to skip.",
				f.display());
		// Could probably do this more efficiently, but...
		inline.clear();
		let _ = io::stdin().read_line(&mut inline)?;
		inline.make_ascii_lowercase();
		if inline.trim() == "s" { continue; }

		'edfile: loop
		{
			// Do it
			edit::edit_file(&mrgf)?;

			// Was it resolved?  Gotta read in the file I guess...
			let fconts = {
				use std::io::Read as _;
				let mut fh = std::fs::File::open(&mrgf)?;
				let mut fc = Vec::with_capacity(fh.metadata()?.len() as usize);
				fh.read_to_end(&mut fc)?;
				fc
			};

			// XXX Lossy str'ing, but it's _probably_ OK because the regex
			// markers are 7-bit ASCII?
			let fcstr = String::from_utf8_lossy(&fconts);
			if cfmarker.is_match(&fcstr)
			{
				loop
				{
					println!("\nConflict markers remain.  'e'dit or 's'kip?");
					inline.clear();
					io::stdin().read_line(&mut inline)?;
					inline.make_ascii_lowercase();
					match inline.trim()
					{
						"s" => continue 'cfiles,
						"e" => continue 'edfile,
						_ => (),
					}
				}
			}

			// Probably resolved.  Double check
			loop
			{
				println!("\nConflict resolved.  Choose action:\n\
						'e'dit again,\n\
						's'kip and discard current resolution,\n\
						'd'iff against current version,\n\
						'D'iff against new upstream version, or\n\
						'a'ccept?");
				inline.clear();
				io::stdin().read_line(&mut inline)?;
				let itstr = inline.trim();
				match itstr
				{
					"a" | "A" => break,
					"s" | "S" => continue 'cfiles,
					"d" | "D" => {
						// fconts is Vec<u8> of the merged result
						let prev_hash = match itstr {
							"d" => cd.cur,
							"D" => cd.new,
							_ => unreachable!("Only 2 chars get here"),
						};
						let prev = {
							let gzf = rtdirs.hashfile(&prev_hash);
							crate::util::compress::decompress_to_vec(&gzf)?
						};

						// XXX shared with show_merges, abstract this...
						use crate::core::merge::merge_diff;
						let dbytes = merge_diff(f, &prev, &fconts);
						let dstr = String::from_utf8_lossy(&dbytes);

						use std::borrow::Cow::{Owned, Borrowed};
						let lstr = match dstr {
							Owned(_)    => "  (include non UTF-8 chars, \
									lossily converted)",
							Borrowed(_) => "",
						};

						println!("diff {}{lstr}\n{dstr}\n", f.display());
						continue;
					},
					"e" | "E" => continue 'edfile,
					_ => println!("Unexpected input {inline}"),
				}
			}

			// Resolved, ready to go ahead.
			break 'edfile;
		}


		// OK, if we got this far, the conflict was presumable resolved
		// and accepted, so save this up, and mark it up as a now-Clean
		// merge.
		//
		// So first, save up the file.
		use crate::util::hash::sha256_file;
		let cleanhash = sha256_file(&mrgf)?;
		let cleangz = format!("{cleanhash}.gz");

		use crate::util::compress::compress_gz;
		let tmpgz = rtdirs.tmp().join(&cleangz);
		compress_gz(&mrgf, &tmpgz)?;

		let finalgz = rtdirs.files().join(&cleangz);
		std::fs::rename(&tmpgz, &finalgz)?;


		// Now, we pull it outta the conflicts and into the cleans
		use crate::core::merge::{Conflict, Clean};
		let cd = conflicts.remove(f).unwrap();
		let Conflict { old, new, cur, res: _ } = cd;

		let res = cleanhash.to_buf();
		let clean = Clean { old, new, cur, res };
		mup.merge_clean.insert(f.to_path_buf(), clean);

		// And this one is fixed!
		fixed += 1;
	}


	// Summarize
	println!("{fixed}/{nconfls} conflicts resolved.");

	// Write out updated state
	rtdirs.state_save(&state)?;

	// Now, did we fix everything?
	let unfixed = nconfls - fixed;
	if unfixed > 0
	{
		println!("{unfixed} conflicts remain; please re-run resolve-merges \
				to resolve.");
		anyhow::bail!("conflicts remain");
	}

	// Yes, we did
	println!("All conflicts resolved.  You may now review the merge results \
			using\n{cmdname} show-merges\nor install the upgrade with\n\
			{cmdname} install");
	Ok(())
}
