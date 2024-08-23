//! $0 upgrade
use std::collections::{HashSet, HashMap};
use std::io::{stdout, Write as _};
use std::path::PathBuf;

use crate::command::CmdArg;
use crate::metadata::MetaFile;
use crate::core::merge;

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

	// Extract out upgrade's own args
	let upargs = match clargs.command {
		crate::command::FrCmds::Upgrade(ua) => ua,
		_ => unreachable!("I'm an upgrade, why does it think I'm not??"),
	};

	// Do the "finalize components" thing, which pulls src outta the list
	// if we don't seem to have src installed.
	config.finalize_components();

	// Show our starting point
	println!("Currently running {version}.");



	/*
	 * Now, the upgrade.
	 *
	 * First, we load up the old/all indices for the currently running
	 * version.
	 */
	println!("Loading info for {version}.");
	let mut server = crate::server::Server::find(&config.servername,
			&version.kernel, &config.keyprint)?;
	server.set_filesdir(rtdirs.files().to_path_buf());
	let metadatas = &["all", "old"];

	print!("Loading metadata index for {version}...");
	stdout().flush()?;
	let mdidx = server.get_metadata_idx()?;
	println!("   OK.");

	print!("Getting metadata files for {version}...  ");
	stdout().flush()?;
	let metamiss = {
		let fd = rtdirs.files();
		let mut missing = mdidx.not_in_dir(fd, metadatas);
		// Include in the old stuff from our saved state in case we need
		// 'em.  Though do we??
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

	print!("Parsing metadata files...  ");
	stdout().flush()?;
	let mut cv_old = mdidx.parse_one_full("old", rtdirs.tmp(), &config)?;
	print!(" old");
	stdout().flush()?;
	let mut cv_all = mdidx.parse_one_full("all", rtdirs.tmp(), &config)?;
	print!(" all");
	stdout().flush()?;
	println!("   OK.");


	// Based on that "old" all file, scan our current system to find out
	// the state of things.
	print!("Inspecting system...  ");
	let scanpaths = {
		let paths = cv_all.allpaths();
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


	// For upgrade, from the current version's INDEX-ALL, we're
	// presumably in a reasonable position to check it against the system
	// and see if there are any components we don't actually have
	// installed.  The main use of this is apparently so the default
	// config works with slightly non-default configs (e.g., no lib32),
	// without having to explicitly be configured for each one.
	//
	// The heuristic f-u.sh uses is ">50% of files", which...  well, it's
	// a heuristic.
	//
	// XXX Maybe we should be doing this on the fetch side as well?
	if true
	{
		println!("Filtering components...");
		let curpaths = cur.allpaths_hashset_nodash();
		let keepcomps = cv_all.components_check(&curpaths);

		let rmcomps: HashSet<_> = cv_all.components().difference(&keepcomps)
				.map(|c| c.clone()).collect();
		let keepcomps: HashSet<_> = keepcomps.into_iter().collect();

		let mut keeps: Vec<_> = keepcomps.iter().map(|c| c.to_string()).collect();
		let mut rms: Vec<_>   = rmcomps.iter().map(|c| c.to_string()).collect();
		keeps.sort_unstable();
		rms.sort_unstable();
		println!("The following components seem to be installed:\n{}",
				&keeps.join(" "));
		if rms.len() > 0
		{
			println!("The following components do NOT seem to be installed:\n\
					{}", &rms.join(" "));

			cv_all.keep_components(&keepcomps);
			cv_old.keep_components(&keepcomps);
		}

		// And update our config for components
		config.components = keepcomps;
	}

	// Don't need the component layer anymore
	let cv_all = cv_all.into_metadata();
	let cv_old = cv_old.into_metadata();

	// f-u.sh seems to collate these together.  I'm not sure why...
	// shouldn't the _all already have the meaningful information anyway?
	// Actually, it doesn't even collate, it just combines, but later
	// uses mostly uniq-ify?
	if true
	{
		let allpaths = cv_all.allpaths_hashset();
		let oldpaths = cv_old.allpaths_hashset();
		let oonld = oldpaths.difference(&allpaths);
		let oocnt = oonld.clone().into_iter().count();
		if oocnt > 0
		{
			eprintln!("I was wrong, there are {oocnt} entries only on old!");
			dbg!(&oonld);
			bail!("Bad programmer, no cookie!");
		}
	}
	let old_server = server;





	/*
	 * Now we do stuff based on the version we're trying to upgrade to.
	 */
	println!("\nLoading info for {}.", upargs.release);
	let mut server = crate::server::Server::find(&config.servername,
			&upargs.release, &config.keyprint)?;
	server.set_filesdir(rtdirs.files().to_path_buf());

	// Only metadata we need from this one is the 'all'.
	let metadatas = &["all"];
	print!("Loading metadata index...");
	stdout().flush()?;
	let mdidx = server.get_metadata_idx()?;
	println!("   OK.");

	print!("Getting metadata files...  ");
	stdout().flush()?;
	let metamiss = mdidx.not_in_dir(rtdirs.files(), metadatas);
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
	print!("Parsing metadata files...  ");
	stdout().flush()?;
	let mut all = mdidx.parse_one_full("all", rtdirs.tmp(), &config)?;
	print!(" all");
	stdout().flush()?;
	println!("   OK.");

	// But prune down to the components we're worrying about, then dump
	// the component level.
	all.keep_components(&config.components);
	let all = all.into_metadata();


	// f-u.sh will replace a non-GENERIC kernel with a GENERIC one, which
	// seems not great.  But, we'll go with it...
	//
	// OTOH, if <basedir> isn't /, then the running kernel doesn't really
	// tell us a darn thing about what should be in the system image
	// we're working on, so just forget it.
	let kconf = crate::info::kernel::conf().unwrap();
	let isroot = config.basedir().to_str() == Some("/");
	if isroot && kconf != "GENERIC"
	{
		println!("\n    WARNING  --  WARNING  --  WARNING");
		println!("This system is running a {kconf} kernel, which is not\n\
				a distributed kernel config.  As part of upgrading, this\n\
				kernel will be replaced with a GENERIC kernel.");
		println!("    WARNING  --  WARNING  --  WARNING\n");
		// If this is duplicated work, it's unimportant
		config.components.insert("kernel/generic".parse().unwrap());
	}


	/*
	 * Rename for the rest of this: "old" is the "all" state of our
	 * currently-running version, and "new" is the "all" state for the
	 * version we're trying to upgrade to.
	 */
	let mut old = cv_all;
	let mut new = all;



	/*
	 * Do various scanning and modifying of our expected work.
	 */
	// If there's anything in the new-version all that wasn't in the
	// current-version all, expand our current system scan results to
	// include it.
	let scanpaths: Vec<_> = {
		let curpaths = cur.allpaths_hashset();
		new.allpaths_hashset().into_iter()
				.filter(|p| !curpaths.contains(p))
				.map(|p| p.to_path_buf()).collect()
	};
	if scanpaths.len() > 0
	{
		println!("{} new paths to scan", scanpaths.len());
		let ncur = scan::scan(config.basedir().to_path_buf(), scanpaths)?;
		{
			// Just for kicks, give details
			let ndir  = ncur.dirs.len();
			let nfile = ncur.files.len();
			let nsl   = ncur.symlinks.len();
			let nhl   = ncur.hardlinks.len();
			let nmiss = ncur.dashes.len();
			println!("Found {ndir} dirs, {nfile} files, {nsl} symlinks, \
					{nhl} hardlinks, and {nmiss} missing files.");
		}
		cur.extend(ncur);
	}


	// Anything that's the same in old and new is stuff we don't need to
	// touch one way or another, so clear it out of everything.
	//
	// Xref down in find_matching() for details about the special case of
	// hardlinks.
	//
	// For an extra special case, consider when old and new may both have
	// the same contents (e.g., the file was updated in (X)p4 and (X+1).
	// But, we're on (X)p3 (not fully fetch'd up on current version
	// patches), so we still have an old version.  It seems like cv_old
	// tends to have that info?  Sometimes?  Though I don't know how it
	// could unless it had multiple lines for a given path, for multiple
	// changes in different (X) patches?  But we've already lost that
	// info 'cuz we're putting it in hashes, so this is kinda
	// best-effort...  I guess we should go with the compromise of "only
	// remove things where all 3 match".
	{
		let ncmatches = new.find_matching(&cur);
		let mut matches = new.find_matching(&old);
		matches.retain(|p| ncmatches.contains(p));
		new.remove_paths(&matches);
		old.remove_paths(&matches);
		cur.remove_paths(&matches);
	}


	// Handle MergeChanges
	let mut to_merge = HashMap::new();
	let dontmerge = merge::dont_merge();
	if config.merge_changes.len() > 0
	{
		// fetch_filter_mergechanges();
		let mc = &config.merge_changes;
		let tm_vold = cv_old.with_filter_paths_regexps(mc);
		let tm_old  = old.with_filter_paths_regexps(mc);
		let tm_cur  = cur.with_filter_paths_regexps(mc);
		let tm_new  = new.with_filter_paths_regexps(mc);

		// Anything in cur that doesn't already match either old or new
		// is a local modification that we're going to attempt to merge.
		// We only try that with files though; the dirs/links are on
		// their own.
		tm_cur.files.iter().for_each(|(p, f)| {
			// Right off the bat, if it's one we don't bother with, don't
			// bother.
			if dontmerge.contains(p) { return; }

			// While f-u.sh does full comparisons and so will include
			// e.g. permission-related mismatches, I'm gonna ignore that
			// and just go with hashes.
			let of = tm_old.files.get(p);
			let nf = tm_new.files.get(p);

			// Hang on, if this isn't in new, WTF are we doing here??
			// And if it's not in old, we can't merge anything anyway...
			let of = match of { Some(f) => f, None => return, };
			let nf = match nf { Some(f) => f, None => return, };

			// If cur matches either hash, we're done
			let ch = f.sha256;
			let oh = of.sha256;
			let nh = nf.sha256;
			if ch == oh || ch == nh { return; }

			// How about the veryold case?  Maybe that triggers in some
			// cases.  When we're "behind" on the patches on our current
			// version, perhaps.
			if let Some(vof) = tm_vold.files.get(p)
			{
				if vof.sha256 == ch { return; }
			}

			// So it's changed, and we have to try merging.  We don't
			// really _know_ what version of the file the user started
			// from, but guess it's the entry from old and we'll run with
			// it.
			to_merge.insert(p.clone(), of.clone());
		});
	}
	let to_merge = to_merge;  // Dump mut JIC
	match to_merge.len()
	{
		nf if nf > 0 => println!("{nf} modified files to merge."),
		_ => (),
	}



	/*
	 * Do various filtering
	 */
	// Handle UpdateIfUnmodified
	let modified_files = {
		use crate::core::filter;
		let ignore: HashSet<_> = to_merge.keys().map(|p| p.as_ref()).collect();
		let mpret = filter::modified_present(&old, &new, &cur,
				&config.update_if_unmodified, Some(&ignore), Some(&cv_old));
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
	// new, so we're already up to date.  In fetch, that's probably a
	// common case.  But for upgrade, if we're actually upgrading (if we
	// weren't, we have bombed out well before here), and there's no
	// changes, something went very very wrong, so this is definitely
	// error-y.
	let relstr = || -> String {
		use crate::info::version::mk_str;
		mk_str(&upargs.release.release, &upargs.release.reltype,
				server.keytag_patchnum())
	};
	if cur.empty()
	{
		let rstr = relstr();
		println!("No updates needed update system to {rstr}.\n\
				I'm an upgrade, so that can't be right, right??");
		bail!("Should have found upgrades to do!");
	}



	/*
	 * Prep up and get all the files we need.
	 */
	println!("");

	// Try and find "clean" old versions for merges.  Of course, this
	// only applies to plain files...
	// f-u.sh fetch_files_premerge()
	let mhashes: Vec<_> = to_merge.iter().filter_map(|(_p, mf)| {
		let hash = mf.sha256.to_buf();
		let hfile = rtdirs.files().join(format!("{hash}.gz"));
		match hfile.is_file() {
			true  => None,
			false => Some(hash),
		}
	}).collect();
	if mhashes.len() > 0
	{
		println!("Trying to fetch {} old files for merging", mhashes.len());

		// In a sense, this feels like it should be best-effort; ideally
		// it should work, but if we can't get any, we can still do the
		// upgrade and punt to the user to handle merging or something.
		// But f-u.sh croaks if anything fails, so I guess we will
		// too...
		use crate::core::pool::hashcheck as hcp;
		use crate::core::hashfetch as hf;

		let tmpdir = rtdirs.tmp().to_path_buf();
		let filesdir = rtdirs.files().to_path_buf();
		let keep = true;
		let ctrl = hcp::Control { tmpdir, filesdir, keep };

		hf::get(&old_server, mhashes, ctrl)?;
	}

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


	// Try getting patches where we can.  See fetch for some discussion
	// of when this does and doesn't apply, and how much it's really
	// worth bothering with.
	//
	// I'm currently disabling this on upgrades, since it seems like the
	// existence of patch files is vanishingly rare; testing a 13.2
	// system upgrade to 13.3 or 14.0 results in ca. 9800 potential
	// patches with 0 actually existing, which means a lot of wasted time
	// getting 404's and adding to server load.
	let maybe_patches: Vec<String> = {
		let pospatches = if false {
				cur.intersect_files_hash(&old)
			} else {
				use std::path::Path;
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

	// Won't trigger here currently; x-ref above
	if maybe_patches.len() > 0
	{
		println!("Trying to fetch {} patch files.", maybe_patches.len());
		let pret = server.fetch_patch_files(maybe_patches,
				rtdirs.tmp().to_path_buf())?;
		println!("Got {} patches.", pret.len());
		if pret.len() > 0
		{
			// Apply
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
		let keep = true;
		let ctrl = hcp::Control { tmpdir, filesdir, keep };

		hf::get(&server, nh, ctrl)?;
	}
	else
	{
		println!("All files present.");
	}



	/*
	 * Now, prep up all the merges.  Assuming we have any.  Maybe we
	 * don't.  Also maybe the moon is green cheese.
	 *
	 * to_merge already contains the old-release MetaFile's.  cur holds
	 * the current system's info, and new the new release's.
	 *
	 * x-ref comment on core::merge about parallelizing.
	 */
	use crate::util::plural;
	let mut merges_clean: HashMap<PathBuf, merge::Clean> = HashMap::new();
	let mut merges_conflict: HashMap<PathBuf, merge::Conflict> = HashMap::new();
	let tmlen = to_merge.len();
	if tmlen > 0
	{
		use std::fs;
		println!("Trying to merge {tmlen} file{}.", plural(tmlen));

		// Prep up a place to do the merges
		let mrgdir = rtdirs.tmp().join("merge");
		fs::create_dir(&mrgdir)?;

		// We do 3-way merges between the old (theoretically pristine
		// from current release), cur (what's on the system now), and new
		// (what's in the new release).  This can yield a couple of
		// possible outcomes, as f-u.sh does it.
		//
		// f-u.sh has a special case: files differ only in RCS tags.  I'm
		// not bothering to implement this, 12.x was the last version
		// that expanded $FreeBSD$, and it's already EOL, so I'm not
		// gonna spend time worrying about that.  And f-u.sh's handling
		// of it doesn't handle the 13-14 case of the keywords going away
		// anyway, so...
		//
		// That aside, how does f-u.sh merge?
		//
		// 1) File doesn't exist in new.  In that case, "merge" is
		//    defined as "buh-bye", so there's no real merging to do
		//    anyway.
		//
		// 2) File doesn't exist in old.  In that case, "merge" is
		//    defined as "just take the new", so...  also not very
		//    merge-y.
		//
		// Those two cases are already handled by us up above when we
		// build to_merge; it already won't have entries for anything
		// that isn't in all 3 of old/cur/new.  Now, "handled" may be a
		// redefinition...   e.g., for case (1), we'd need to explicitly
		// check and do that separately, but I think I'll just plan to
		// leave it alone.  If a file went away in old->new, it's
		// presumably no longer important, but you made local changes...
		// and this only triggers on MergeChanges...  well, we'll just
		// leave your local file alone.
		//
		// Ahem.  Where were we?
		//
		// 3) A few files (derived db's like passwd) we just ignore.  So,
		//    still no actual merging.
		//
		// 4) And then files we 3-way merge.  Either they succeed, in
		//    which case we _show_ the user, but don't provide any way to
		//    override, or they fail, in which case we dump them in
		//    $EDITOR in a diff3-style file to fix.
		for (path, of) in to_merge
		{
			use merge::{merge_files, MergeError};
			use crate::util::compress;
			use crate::util::hash::sha256_file;

			// of contains old's data for this, so we need cur's and
			// new's.  Should be impossible for these to fail; we went
			// over cur to build to_merge in the first place, and if
			// there weren't a new entry, we wouldn't have added it.
			let cf = cur.files.get(&path).unwrap();
			let nf = new.files.get(&path).unwrap();

			// Read in the contents.  It's actually possible that cur
			// might already exist decompressed from stashing, so we
			// could save a few cycles by checking for it there first,
			// but I won't bother for now.
			let mf_data = |m: &MetaFile| -> Result<_, _> {
				let hb = m.sha256.to_buf();
				let gzf = rtdirs.hashfile(&hb);
				compress::decompress_to_vec(&gzf)
			};
			let oldb = mf_data(&of)?;
			let curb = mf_data(cf)?;
			let newb = mf_data(nf)?;

			// Merge into a temp file, then figure out what to do.
			//
			// If it went OK, put it in our clean list and move on.  If
			// it got an IO error, just bomb out.
			let mut outf = tempfile::NamedTempFile::new_in(rtdirs.tmp())?;
			let mret = merge_files(&oldb, &curb, &newb, outf.as_file_mut());
			let isok = match mret {
				Ok(_) => true,
				Err(e) => match e {
					// IO errors we bomb, Conflicts we continue on
					MergeError::IO(ioe) => return Err(ioe)?,
					MergeError::Conflicts => false,
				},
			};

			// OK, no matter what we'll want the hash and to stash it up
			let nhb = sha256_file(outf.path())?.to_buf();
			let savepath = rtdirs.files().join(format!("{nhb}.gz"));
			compress::compress_gz(outf.path(), &savepath)?;

			// Now, if it was OK, we store up the hash of both the
			// current system file, and the merged result, to show that
			// diff.  If it wasn't OK, the conflicted file hash gets
			// stored for later resolution.
			let pbuf = path.to_path_buf();
			let old = of.sha256.to_buf();
			let new = nf.sha256.to_buf();
			let cur = cf.sha256.to_buf();
			match isok
			{
				true => {
					let res = nhb;
					let cm = merge::Clean { old, new, cur, res };
					merges_clean.insert(pbuf, cm);
				},
				false => {
					let res = nhb;
					let cm = merge::Conflict { old, new, cur, res };
					merges_conflict.insert(pbuf, cm);
				},
			}
		}

		let oklen = merges_clean.len();
		let cflen = merges_conflict.len();
		if oklen > 0
		{
			println!("{oklen} file{} merged cleanly.\n\
					Run `{cmdname} show-merges` to review.", plural(oklen));
		}
		if cflen > 0
		{
			println!("{cflen} file{} couldn't be automatically merged.\n\
					Run `{cmdname} resolve-merges` to manually resolve.",
					plural(cflen));
		}
	}
	let has_merges    = !merges_clean.is_empty();
	let has_conflicts = !merges_conflict.is_empty();



	// We've done all the prep we can do now.  Stash up our state, and
	// tell the user to move on.  Roughly f-u.sh's
	// fetch_create_manifest().
	//
	// Unlike f-u.sh, we aren't doing the conflict resolution here, but
	// outsourcing that to a separate command.  It will need to be done
	// before the install can be upgraded; we'll remind the user here,
	// but later commands like install will check before doing their
	// thing.

	// First, we'll store up the manifest.
	use crate::state::Manifest;
	let manifest = {
		// The new version is what we asked for, maybe with a patch from
		// the server keytag.
		let mut vers = upargs.release.clone();
		vers.patch = server.keytag_patchnum();
		Manifest::new_upgrade(cur, new, vers, merges_clean, merges_conflict)
	};


	// Print out a summary.  No details, 'cuz we don't wanna own the
	// terminal and do pagers and such; x-ref fetch command for longer
	// comment.  Other commands can show details.
	{
		let sum = manifest.change_summary();
		let rem = sum.removed.len();
		let add = sum.added.len();
		let upd = sum.updated .len();
		println!("\nUpgrade will remove {rem} files, add {add} files, and \
				update {upd} files.");
	}


	// Save up that state
	state.manifest = Some(manifest);
	let save_mdidx = mdidx.clone_matching(metadatas);
	state.meta_idx = Some(save_mdidx);
	rtdirs.state_save(&state)?;


	// Remind the user if there are conflicts to resolve.  Otherwise just
	// tell 'em it's ready to go.
	if has_merges
	{ println!("Run `{cmdname} show-merges` to review merge results."); }
	println!("Run `{cmdname} show-install` to see details of what will be \
			installed.");
	if has_conflicts
	{
		println!("CONFLICTS PRESENT: Conflicts must be resolved with \
				`{cmdname} resolve-merges` before upgrade can be installed.");
	}
	else
	{
		let rstr = relstr();
		println!("\nRun `{cmdname} install` to upgrade from {version} to \
				{rstr}.");
	}

	// Maybe there's an EOL warning?
	if let Some(ew) = old_server.eol_warning(&version) { println!("\n{ew}"); }


	// And that's it...
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

	// Nonsensical to upgrade to ourself
	use crate::command::FrCmds as FC;
	match &carg.clargs.command
	{
		FC::Upgrade(ua) => {
			// Tweak away a -p's on the versions
			let mut curv = carg.version.max().clone();
			curv.patch = None;
			let mut upv = ua.release.clone();
			upv.patch = None;

			if curv == upv
			{
				let es = format!("Cannot upgrade from {curv} to itself.");
				errs.push(es);
			}
		},
		_ => unreachable!("This is an upgrade, why am I not an upgrade?!?"),
	}


	match errs.len() {
		0 => Ok(()),
		_ => {
			use anyhow::anyhow;
			let estr = anyhow!("Cannot run upgrade::\n  - {}",
					errs.join("\n  - "));
			Err(estr)
		},
	}
}
