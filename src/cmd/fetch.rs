//! $0 fetch
use std::collections::HashSet;
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

	// See what sorta state we're in, and if it's one where we shouldn't
	// be running fetch.
	let mut state = rtdirs.state_load()?;

	// I'm gonna need to know my command name in a few places, so just
	// pre-figure it...
	let cmdname = crate::util::cmdname();

	// If we're showing kernel installed, that means there _is_ an
	// upgrade in progress, but not done (or there wouldn't be any
	// state), so...
	if state.upgrade_in_progress()
	{
		eprintln!("Partially completed upgrade already in progress.  \
				Perhaps you need to run `{cmdname} install` to finish.\n\
				Or run `{cmdname} clean --pending` to discard state.");
		bail!("upgrade in progress");
	}


	// OK, bust it up so we can move the bits around individually.
	let CmdArg { clargs, mut config, version } = carg;
	let _ = clargs;  // Until we need 'em

	// Do the "finalize components" thing, which pulls src outta the list
	// if we don't seem to have src installed.
	config.finalize_components();

	// Fetch will use the INDEX-{NEW,OLD} metadata thingies
	let metadatas = &["new", "old"];

	// Show our starting point
	println!("Currently running {version}.");


	/*
	 * Now we can start the actual fetch process.  First, find a server
	 * we can talk to.
	 */
	let mut server = crate::server::Server::find(&config.servername,
			&version.kernel, &config.keyprint)?;

	// Set copy of dirnames the server object accesses internally
	server.set_filesdir(rtdirs.files().to_path_buf());



	/*
	 * Next, get metadata from it
	 */
	// Load up the metadata index stuff
	print!("Loading metadata index...");
	stdout().flush()?;
	let mdidx = server.get_metadata_idx()?;
	println!("   OK.");

	// Find and fetch any metadata patches we need.  The logic around
	// this is pretty thoroughly opaque, I'm not quite clear on what it's
	// trying to do yet.  It looks like we need to have done updates and
	// have more updates to apply for it to mean something?  Also, maybe
	// it's "just" an optimization for the followup...
	// print!("Fetching metadata patches...  TODO\n");
	// let metapatches = some::long::path::to::figure::out();

	// Find and download any missing metadata files
	print!("Getting all metadata files...  ");
	stdout().flush()?;
	let metamiss = {
		let fd = rtdirs.files();
		let mut missing = mdidx.not_in_dir(fd, metadatas);
		if let Some(md) = state.meta_idx
		{ missing.extend(md.not_in_dir(fd, metadatas)); }
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


	// f-u.sh has 'sanity checks' of the metafiles.  We do actual full
	// parses, so they aren't functionally needed.  And parsing is
	// so fast, there's no useful gain from doing cheaper checks first
	// either.


	// Parse out the metadatas from each
	print!("Parsing metadata files...  ");
	stdout().flush()?;
	let old = mdidx.parse_one_full("old", rtdirs.tmp(), &config)?;
	print!(" old");
	let new = mdidx.parse_one_full("new", rtdirs.tmp(), &config)?;
	print!(" new");

	println!("   OK.");

	// Strip those MetadataGroup's into Metadata's.  Revisit this if we
	// decide to do the component-heuristic stuff here.
	let mut old = old.into_metadata();
	let mut new = new.into_metadata();



	/*
	 * Now scan over the system looking at the files names in our indices
	 * and seeing what their current status is.
	 */
	print!("Inspecting system...  ");
	let scanpaths = {
		let mut paths = HashSet::new();
		for md in [&old, &new].iter()
		{
			let prefs = md.allpaths();
			paths.extend(prefs.into_iter());
		}

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
	// our cur = f-u.sh's INDEX-PRESENT
	let mut cur = scan::scan(config.basedir().to_path_buf(), scanpaths)?;
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
	 * Do various filtering
	 */
	let modified_files = {
		// fetch_filter_unmodified_notpresent()
		use crate::core::filter;
		let mpret = filter::modified_present(&old, &new, &cur,
				&config.update_if_unmodified, None, None);
		// This returns what f-u.sh calls "modifiedfiles"
		filter::apply_modified_present(mpret, &mut old, &mut new, &mut cur)
	};
	match modified_files.len()
	{
		nf if nf > 0 => println!("{nf} modified files will be ignored."),
		_ => (),
	}

	// AllowAdd and AllowDelete handling would go here

	// Handle KeepModifiedMetadata.  Anything where the current metadata
	// differs from old, replace new's metadata with our stuff.
	if config.keep_modified_metadata
	{
		let modd = cur.modified_metadata(&old);
		if !modd.empty()
		{
			new.replace_metadata_from(&modd);
		}
	}

	// Now collate cur/new together, and remove any lines that are
	// the same between them.  f-u.sh's fetch_filter_uptodate()
	{
		let ntmp = new.clone();
		new.remove_matching(&cur);
		cur.remove_matching(&ntmp);
	}


	// If there's nothing left in new at this point, that means cur ==
	// new, so we're already up to date.
	let relstr = || -> String {
		use crate::info::version::mk_str;
		mk_str(&version.kernel.release, &version.kernel.reltype,
				server.keytag_patchnum())
	};
	if new.empty()
	{
		let rstr = relstr();
		println!("\nNo updates needed to update system to {rstr}");
		// XXX x-ref noup in cron::run() if you change this string.

		// But give an EOL warning if there is one.
		if let Some(ew) = server.eol_warning(&version) { println!("\n{ew}"); }

		return Ok(());
	}



	/*
	 * Prep up and get all the files we need.
	 */

	// Be sure any of our cur files are stashed up in the filesdir.  Any
	// of them that are unmodified from old, we may need for patching.
	// The modified ones don't fall into that, but may be needed for
	// rollback, so we'll just stash 'em all.
	if let Some(stashfiles) = cur.files_no_hash_dir(rtdirs.files())
	{
		println!("Stashing {} current files.", stashfiles.len());
		cur.stash_files(&stashfiles, config.basedir().to_path_buf(),
				rtdirs.tmp().to_path_buf(), rtdirs.files().to_path_buf())?;
	}


	// Try getting patches where we can.  In principal, that's any case
	// where we have some applicable <before>.gz and new needs an
	// <after>.gz.  In practical, like f-u.sh, we're only going to
	// consider the cases where old/cur match (and so we just stashed up
	// the unmodified files above) and the matching filename in new.
	// This leaves some potential on the table perhaps, but then,
	// patching is really just a bandwidth optimization from the servers.
	//
	// I wonder how much it really saves even, in 202x bandwidth.
	// clang/llvm and debug files can be a hundred megs or so, but short
	// of that, everything else gz's down to a dozen megs-ish.  Well,
	// what the heck...
	//
	// Stubbed out at the moment, since I don't have any handling for
	// these if we get 'em and no good way to test it.
	let maybe_patches: Vec<String> = {
		let pospatches = if true {
				cur.intersect_files_hash(&old)
			} else {
				use std::collections::HashMap;
				use std::path::Path;
				use crate::metadata::MetaFile;
				let pp: Option<HashMap<&Path, &MetaFile>> = None;
				pp
			};
		match pospatches {
			Some(pf) => { pf.into_iter().filter_map(|(path, mf)| {
					let nmf = new.files.get(path)?;

					// Shouldn't be possible, but...
					if nmf.sha256 == mf.sha256 { return None; }

					// If we don't have the 'old' hashfile, there's
					// nothing to patch.
					let mfh = mf.sha256.to_buf();
					let ohgz = format!("{}.gz", mfh);
					let ohfile = rtdirs.files().join(ohgz);
					if !ohfile.is_file() { return None; }

					// If we already have the 'new' hashfile, we also
					// don't need to patch anything.
					let nmfh = nmf.sha256.to_buf();
					let nhgz = format!("{}.gz", nmfh);
					let nhfile = rtdirs.files().join(nhgz);
					if nhfile.is_file() { return None; }

					// OK, this is a possible patch then.  Patches are
					// _not_ gz'd, apparently?
					Some(format!("{}-{}", mf.sha256.to_buf(),
							nmf.sha256.to_buf()))
				}).collect()
			},
			None => Vec::new(),
		}
	};

	// Try getting and applying any we can.
	if maybe_patches.len() > 0
	{
		println!("Trying to fetch {} patch files.", maybe_patches.len());
		let pret = server.fetch_patch_files(maybe_patches,
				rtdirs.tmp().to_path_buf())?;
		println!("Got {} patches.", pret.len());
		if pret.len() > 0
		{
			// Now let's try applying them...
			use crate::core::pool::patch as pp;
			let tmpdir = rtdirs.tmp().to_path_buf();
			let filesdir = rtdirs.files().to_path_buf();
			let keep = true;
			let ctrl = pp::Control { tmpdir, filesdir, keep };

			use crate::core::patchcheck as pc;
			let _okpatches =  pc::patch(pret, ctrl)?;
		}
	}


	// What hashes might we still need?  That would be anything in new
	// that isn't already on the system (which we already did above when
	// pulling out the matching entries from cur), and that we don't have
	// a <hash>.gz for.
	let needhashes = new.hashes_no_hash_dir(rtdirs.files());
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
		let keep = false; // Not currently reprocessing
		let ctrl = hcp::Control { tmpdir, filesdir, keep };

		hf::get(&server, nh, ctrl)?;
	}
	else
	{
		println!("All files present.");
	}


	// OK, mention the modified_files if we found any.
	//
	// XXX Should we just try to merge like upgrade?  Investigate...
	if !modified_files.is_empty()
	{
		let ml = modified_files.len();
		println!("The following {ml} files are affected by updates.  No\n\
				changes have been downloaded, however, because the files have\n\
				been modified locally:\n");

		// Huh, you'd think there'd be a From<HashSet> for Vec...
		let mut mfv: Vec<_> = modified_files.iter()
				.map(|p| p.to_string_lossy()).collect();
		mfv.sort_unstable();
		println!("{}\n\n", mfv.join("\n"));
	}


	// Now stash up the todo list into our state for the install step.
	// Roughly, f-u.sh's fetch_create_manifest().
	use crate::state::Manifest;
	let manifest = {
		// Build version from our current running version, with the patch
		// from the server's keytag.
		let mut vers = version.max().clone();
		vers.patch = server.keytag_patchnum();
		Manifest::new_fetch(cur, new, vers)
	};

	// Print out a summary.  We don't display the full list like f-u.sh
	// does, 'cuz we don't want to own the terminal enough to spawn off
	// pagers etc.  We can trivially add a command to display the
	// details.
	{
		let sum = manifest.change_summary();
		let rem = sum.removed.len();
		let add = sum.added.len();
		let upd = sum.updated.len();
		println!("\nUpgrade will remove {rem} files, add {add} files, and \
				update {upd} files.\n\
				Run `{cmdname} show-install` for details.");
	}

	// Prep it up for saving
	state.manifest = Some(manifest);

	// Stash up the metafiles from this run; f-u.sh calls this
	// 'tINDEX.present'
	let save_mdidx = mdidx.clone_matching(metadatas);
	state.meta_idx = Some(save_mdidx);

	// OK, save up that state
	rtdirs.state_save(&state)?;

	// And we're done.  If we get this far, there's something to install,
	// so remind the user.
	let rstr = relstr();
	println!("\nRun `{cmdname} install` to upgrade from {version} to {rstr}.");

	// Also give an EOL warning if there is one.
	if let Some(ew) = server.eol_warning(&version) { println!("\n{ew}"); }

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
			let estr = anyhow!("Cannot run fetch::\n  - {}",
					errs.join("\n  - "));
			Err(estr)
		},
	}
}
