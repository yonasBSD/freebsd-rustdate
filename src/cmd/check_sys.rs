//! $0 check-sys
use std::collections::{HashSet, HashMap};
use std::io::{stdout, Write as _};

use crate::command::CmdArg;

use anyhow::bail;


pub(crate) fn run(carg: CmdArg) -> Result<(), anyhow::Error>
{
	// Check our various config etc.
	check(&carg)?;

	// Setting up various dirs
	let rtdirs = crate::core::RtDirs::init(&carg.config.basedir(),
			&carg.config.workdir())?;

	// No state for this cmd

	// I'm gonna need to know my command name in a few places, so just
	// pre-figure it...
	//let cmdname = crate::util::cmdname();


	// OK, bust it up so we can move the bits around individually.
	let CmdArg { clargs, mut config, version } = carg;

	// For check-sys, we use the IDSIgnorePaths, rather than IgnorePaths.
	// The simplest way is the hacky way...
	config.ignore_paths = config.ids_ignore_paths.clone();

	// We always want to know about ownership differences in files, even
	// when we're not root.  Actually, we may not be, but we need to do
	// that filtering later in the process anyway.
	crate::metadata::set_ugid_cmp(true);

	// Do the "finalize components" thing, which pulls src outta the list
	// if we don't seem to have src installed.
	config.finalize_components();

	// Show our starting point
	println!("Currently running {version}.");

	// Extract args
	let args = match clargs.command {
		crate::command::FrCmds::CheckSys(a) => a,
		_ => unreachable!("I'm a check-sys, why does it think I'm not??"),
	};


	// Find the server
	let mut server = crate::server::Server::find(&config.servername,
			&version.kernel, &config.keyprint)?;

	// Set copy of dirnames the server object accesses internally
	server.set_filesdir(rtdirs.files().to_path_buf());



	/*
	 * Load the metadata
	 */
	// Load up the metadata index stuff
	print!("Loading metadata index...");
	stdout().flush()?;
	let mdidx = server.get_metadata_idx()?;
	println!("   OK.");

	// All we need here is the INDEX-ALL
	let metadatas = &["all"];

	// Get the one we need
	print!("Getting all metadata files...  ");
	stdout().flush()?;
	let metamiss = {
		let fd = rtdirs.files();
		let missing = mdidx.not_in_dir(fd, metadatas);
		missing
	};
	match metamiss.len()
	{
		0 => println!("All present."),
		_ => {
			println!("{} missing.", metamiss.len());

			// So grab 'em.
			println!("Fetching...");
			let files = metamiss.into_iter().collect();
			server.fetch_metafiles(files)?;
			println!("Done.");
		},
	};

	// Check all the metafiles hashes
	print!("Checking metadata file hashes...  ");
	stdout().flush()?;
	let hres = {
		let fd = rtdirs.files();
		let td = rtdirs.tmp();
		mdidx.check_hashes(fd, td, metadatas)
	};
	match hres {
		Ok(_) => println!("   OK."),
		Err(e) => {
			println!("   Errors found.\n{}", e.join("\n"));
			bail!("Invalid metafiles, bailing.");
		},
	};


	// Parse out the metadata
	print!("Parsing metadata files...  ");
	stdout().flush()?;
	let mut all = mdidx.parse_one_full("all", rtdirs.tmp(), &config)?;
	print!(" all");
	println!("   OK.");


	// Handle path in/exclusions, if there are any.
	if args.paths.len() > 0 || args.exclude.len() > 0
	{
		let mut npaths = all.len();
		println!("\nFiltering paths: {npaths} originally.");

		if args.paths.len() > 0
		{
			all.keep_paths_matching(&args.paths);
			npaths = all.len();
			println!("{npaths} retained from --paths");
		}

		if args.exclude.len() > 0
		{
			all.remove_paths_matching(&args.exclude);
			let npaths2 = all.len();
			let excl = npaths - npaths2;
			println!("{excl} excluded via --exclude");
		}

		println!("");
	}

	// While we're at it, we can skip the hashing during the scan if
	// we're ignoring hashes.
	let do_hashes = {
		let hash = crate::command::CheckSysIgnore::Hash;
		!args.ignore.contains(&hash)
	};


	// Scan the system
	print!("Inspecting system...  ");
	let scanpaths = {
		let paths = all.allpaths();
		let mut paths: Vec<_> = paths.into_iter()
				.map(|p| p.to_path_buf()).collect();
		paths.sort_unstable();
		paths
	};
	if scanpaths.len() == 0
	{
		// ...  doesn't seem likely...
		println!("\nNo paths to scan found?!  Dunno what to do...");
		bail!("No paths to scan");
	}
	println!("{} paths to scan", scanpaths.len());
	use crate::core::scan;
	let mut cur = scan::scan_inner(config.basedir().to_path_buf(),
			scanpaths, do_hashes)?;
	{
		// Just for kicks, give details
		let ndir  = cur.dirs.len();
		let nfile = cur.files.len();
		let nsl   = cur.symlinks.len();
		let nhl   = cur.hardlinks.len();
		let nmiss = cur.dashes.len();
		println!("Found {ndir} dirs, {nfile} files, {nsl} symlinks, \
				{nhl} hardlinks, and {nmiss} missing files.");
	}


	// Filter components.
	//
	// XXX Same as in upgrade, we should abstract this better...
	if true
	{
		println!("\nFiltering components...");
		let curpaths = cur.allpaths_hashset_nodash();
		let keepcomps = all.components_check(&curpaths);

		let rmcomps: HashSet<_> = all.components().difference(&keepcomps)
				.map(|c| c.clone()).collect();
		let keepcomps: HashSet<_> = keepcomps.into_iter().collect();

		let mut keeps: Vec<_> = keepcomps.iter().map(|c| c.to_string()).collect();
		let mut rms: Vec<_>   = rmcomps.iter().map(|c| c.to_string()).collect();
		keeps.sort_unstable();
		rms.sort_unstable();
		println!("The following components seem to be installed:\n  {}",
				&keeps.join(" "));
		if rms.len() > 0
		{
			println!("The following components do NOT seem to be installed:\n  \
					{}", &rms.join(" "));

			all.keep_components(&keepcomps);
		}
		println!("");

		// And update our config for components
		config.components = keepcomps;
	}

	// Now we don't need the component level anymore
	let mut all = all.into_metadata();



	/*
	 * For the purposes of check, we're ignoring the IDSIgnorePaths too
	 * (x-ref above), but all the "keep modified" stuff we explicitly
	 * ignore.
	 */

	// Now, anything that matches between cur and all is stuff that...
	// y'know.  Matches.
	print!("Comparing...  ");
	stdout().flush()?;
	{
		let atmp = all.clone();
		all.remove_matching_checksys(&cur);
		cur.remove_matching_checksys(&atmp);

		// Also due to the component removal, we need to catch up and
		// remove those (presumably lots of dash lines) from cur.
		cur.keep_paths(&all.allpaths_hashset());

	}
	println!("Done.");




	// If there's nothing left in all, everything's the same.
	let relstr = || -> String {
		use crate::info::version::mk_str;
		mk_str(&version.kernel.release, &version.kernel.reltype,
				server.keytag_patchnum())
	};
	if all.empty()
	{
		let rstr = relstr();
		println!("\nNo differences found vs. {rstr}.");

		return Ok(());
	}


	// OK, there are differences.  Are there some types we'll ignore?
	let itypes: HashSet<&str> = args.ignore.iter()
			.map(|i| i.as_ref()).collect();
	let mut igns: HashMap<&str, u32> = itypes.iter().map(|t| (*t, 0)).collect();
	let mut should_ignore = |t: &str| -> bool {
		match itypes.contains(t) {
			true => {
				let e = igns.get_mut(t).expect("Must be ignored type");
				*e += 1;
				true
			},
			false => false,
		}
	};

	// Rack up what they are
	let allpaths = all.allpaths_hashset_nodash();
	let len = allpaths.len();
	let mut diffs: HashMap<&std::path::Path, Vec<String>> = HashMap::with_capacity(len);
	for p in allpaths
	{
		let mut add = |s| { diffs.entry(p).or_default().push(s); };

		// Load 'em up, but cut out early if it's just nonexistent in
		// cur.
		let up = all.get_path(p).expect("Must be in all");
		let my = match cur.get_path(p) {
			Some(m) => m,
			None => {
				if !should_ignore("missing")
				{ add(format!("doesn't exist on your system")); }
				continue;
			},
		};

		// If the types are different, that's also the end of it.
		let utype = up.ftype();
		let mtype = my.ftype();
		if utype != mtype
		{
			if !should_ignore("type")
			{
				add(match mtype
				{
					"dashline" => format!("is missing but should be a {utype}"),
					_ => format!("is a {mtype} but should be a {utype}"),
				})
			}
			continue;
		}

		// OK, we know they're the same type now, so we can get the diffs
		// just for that type.
		let diffs = match my.diff(&up) {
			Err((mt, ut)) => {
				// Differing type; this should be impossible
				unreachable!("Shouldn't be possible to compare {mt} with {ut}");
			},
			Ok(d) => d,
		};
		if let Some(diffs) = diffs
		{
			diffs.into_iter().for_each(|d| {
				// We may be doing some filtering down of what types of
				// differences we care about.
				if !should_ignore(d.dtype())
				{ add(d.to_string()); }
			});
		}
	}


	/*
	 * OK, what'd we find?
	 */
	use itertools::Itertools as _;

	// Did we ignore anything?  Mention at least.
	let nigns: u32 = igns.values().sum();
	if nigns > 0
	{
		let mut istrs = Vec::new();
		for t in igns.keys().sorted()
		{
			let num = igns[t];

			// Mildly hackish: if we're ignoring the hash, we wouldn't
			// have scanned any of them, so they'll all "differ".  Just
			// mention that we're ignoring hashes, a count is stupid.
			if num > 0 && *t == "hash"
			{ istrs.push("hash".to_string()); continue; }

			if num > 0 { istrs.push(format!("{t}({num})")) }
		}
		println!("Ignored differing {}.\n", istrs.join(", "));
	}

	// Now the remaining details
	match diffs.len()
	{
		0 => println!("No differences found."),
		n => {
			println!("{n} difference{} found:", crate::util::plural(n));
			for (p, pdiffs) in diffs.iter().sorted()
			{
				let pdis = p.display();
				pdiffs.iter().for_each(|d| println!(" {pdis} {d}"));
			}
		},
	}

	Ok(())
}


/// Do some checks of our config/etc
fn check(carg: &CmdArg) -> Result<(), anyhow::Error>
{
	let mut errs: Vec<String> = vec![];

	macro_rules! check {
		( $fld:ident) => {
			match crate::check::$fld(&carg.config) {
				Ok(_) => (),
				Err(e) => errs.push(e),
			}
		};
	}

	// Lot of simple config fields that have common check types
	check!(servername);
	check!(keyprint);
	check!(workdir);
	check!(basedir);

	// Should only run on releases (temporarily knocked off for dev)
	match crate::check::version(&carg.version) {
		Ok(_) => (),
		Err(_e) => (), // errs.push(e),
	};


	match errs.len() {
		0 => Ok(()),
		_ => {
			use anyhow::anyhow;
			let estr = anyhow!("Cannot run check-sys::\n  - {}",
					errs.join("\n  - "));
			Err(estr)
		},
	}
}
