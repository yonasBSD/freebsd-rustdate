//! $0 extract
use std::collections::HashSet;
use std::io::{stdout, Write as _};
use std::path::Path;

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


	// Extract args
	let args = match clargs.command {
		crate::command::FrCmds::Extract(a) => a,
		_ => unreachable!("I'm a extract, why does it think I'm not??"),
	};
	let dry = args.dry_run;


	// If we're in regex mode, we need to transform the given path(s) to
	// regexes.  We should probably make sure there are paths anyway,
	// too...
	if args.paths.len() == 0
	{
		eprintln!("\nNo paths given to extract.");
		bail!("extract needs paths");
	}

	let path_res = match args.regex {
		true => {
			use regex_lite::Regex;
			let mut pregs = Vec::with_capacity(args.paths.len());

			for p in &args.paths
			{
				let pstr = match p.to_str() {
					Some(s) => s,
					None => {
						bail!("Path {} isn't valid UTF-8", p.to_string_lossy());
					},
				};
				let pre = match Regex::new(pstr) {
					Ok(re) => re,
					Err(e) => {
						bail!("Error: '{}' isn't a valid regex: {e}",
								p.to_string_lossy());
					}
				};
				pregs.push(pre);
			}

			pregs
		}
		false => Vec::new(),
	};



	// Show our starting point
	println!("Currently running {version}.");

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


	// Unlike most other commands, we're only conditionally trimming
	// components.
	//
	// If we are filtering them, we need to do a full (quick) scan, just
	// to see what's around.  We'll do a more detailed scan later to to
	// compare just the files we expect to overwrite (or not).
	use itertools::Itertools as _; // .sorted()
	if args.only_components
	{
		print!("Scanning system for components check...  ");
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
		let cur = scan::scan_inner(config.basedir().to_path_buf(),
				scanpaths, false)?;
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

		println!("\nFiltering components...");

		// The src thing
		config.finalize_components();

		// Now compare to the scan
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
	else
	{
		let keeps = &config.components;
		println!("\nUsing all config-specified components:\n  {}",
				keeps.iter().sorted().join(" "));
		all.keep_components(&keeps);
	}


	// Now we don't need the component level anymore, one way or another.
	let mut all = all.into_metadata();

	// And we never need dash lines anyway.
	all.dashes = HashSet::new();


	/*
	 * We're not using any of the IgnorePaths or anything here; this is
	 * explicitly a manual use, do-what-I-say command.
	 */


	/*
	 * OK, now let's see what we're expecting to install...
	 */
	print!("\nMatching paths...  ");
	stdout().flush()?;

	if args.regex
	{
		// Check against our RE's
		all.filter_paths_regexps(&path_res);
	}
	else
	{
		// String comparison
		let paths: HashSet<&Path> = args.paths.iter()
				.map(|p| p.as_ref()).collect();
		all.keep_paths(&paths);
	}

	use crate::util::plural;
	let npaths = all.len();
	let npp = plural(npaths);
	println!("   done: {npaths} path{npp} matched.");

	if npaths == 0
	{
		println!("\nNo matching paths found.");
		return Ok(());
	}



	/*
	 * Now we know what paths we may be extracting, we can do a more
	 * detailed scan to tell the user something about what's happening.
	 */
	println!("Inspecting {npaths} path{npp}.");
	use crate::core::scan;
	let ipvec = all.allpaths().iter().map(|p| p.to_path_buf()).collect();
	let cur = scan::scan(config.basedir().to_path_buf(), ipvec)?;
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


	/*
	 * If we're not in force mode, we don't overwrite things that already
	 * match.
	 */
	if !args.force
	{
		print!("Removing unchanged entries...  ");
		stdout().flush()?;
		all.remove_matching(&cur);

		let rlen = all.len();
		println!("{rlen} path{} remaining.", plural(rlen));

		if rlen == 0
		{
			println!("Nothing left to do.");
			return Ok(());
		}
	}


	/*
	 * Fetch necessary hashes
	 */
	let mut needhashes = all.hashes_no_hash_dir(rtdirs.files());
	if dry
	{
		if let Some(nh) = needhashes
		{
			let nh = nh.len();
			println!("DRY RUN: {nh} file{} would need to be downloaded.",
					plural(nh));
			needhashes = None;
		}
	}
	if let Some(nh) = needhashes
	{
		// All encapsulated up, just build the control with the dirs and
		// kick it off.
		use crate::core::pool::hashcheck as hcp;
		use crate::core::hashfetch as hf;

		// It's a little layer-violate-y that we expose this up here, but
		// we need to get all this info in anyway, so might as well
		// package it up.
		let tmpdir = rtdirs.tmp().to_path_buf();
		let filesdir = rtdirs.files().to_path_buf();
		let keep = false;
		let ctrl = hcp::Control { tmpdir, filesdir, keep };

		hf::get(&server, nh, ctrl)?;
	}
	else
	{
		println!("All data files present.");
	}
	println!("");


	/*
	 * Now we know everything about what we want to do.  If we're dry
	 * running, just say it.  Else, do it.
	 */

	if dry
	{
		println!("DRY RUN: Would install the following:");
		let mut paths = all.allpaths();
		paths.sort_unstable();
		for p in paths { println!("  {}", p.display()); }
		return Ok(());
	}

	// Reuse bits from install
	use crate::core::install;
	println!("Installing files");
	let isplit = all.into_split_types();
	install::split(isplit, &rtdirs, config.basedir(), false)?;

	println!("\nDone.");
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
			let estr = anyhow!("Cannot run extract::\n  - {}",
					errs.join("\n  - "));
			Err(estr)
		},
	}
}
