//! Filesystem scanning
use std::path::PathBuf;
use std::collections::{HashMap, HashSet};

use crate::metadata::Metadata;

/// Scan for a set of pathnames under a dir to load up info about them.
/// Mostly used to compare against Metadata from a server.
///
/// XXX Re-imagine how paths works.  If we really _will_ always get owned
/// pathbufs, maybe we should try replumbing so we just use and move them
/// out all the way down?
pub(crate) fn scan(basedir: PathBuf, paths: Vec<PathBuf>)
		-> Result<Metadata, anyhow::Error>
{
	scan_inner(basedir, paths, true)
}


/// Lower-level scan func, when more control is needed.
pub(crate) fn scan_inner(basedir: PathBuf, paths: Vec<PathBuf>, hash: bool)
		-> Result<Metadata, anyhow::Error>
{
	// First, kick off a pool of scanners to rack up info about all these
	// files
	use crate::core::pool::scan as pool;

	// Build up the pool
	// XXX Be less dumb about nthreads
	let ctrl = pool::Control { basedir, hash, ..Default::default() };
	let sp = pool::Scan::new(paths.len());

	// Send it
	let scanres = {
		use crate::core::pool::Pool as _;
		sp.run(&ctrl, paths)?
	};

	// Split it up for easier access.
	let pool::PoolResult { mut oks, missing, errs } = scanres;


	// If there were any _error_ errors, we should just expect to fail.
	if let Some(errs) = errs { return Err(errs)?; }

	// OK, now go through those results and sort them out into a form
	// that's useful for our caller.  'errs' we already handled.

	// The ok's well have to break out into the various File/Dir/etc
	// types.  We probably want the outputs to be sorted, but we
	// definitely want the hardlink order to be deterministic, and the
	// metafiles f-u builds seems to be asciibetical, so...
	oks.sort_unstable_by(|a, b| a.path.cmp(&b.path));

	// Track hardlink possibilities.  If something doesn't have nlink>1,
	// it can't be a hardlink, so only relatively few things will end up
	// here...
	//                       dev, ino
	let mut hlinks: HashMap<(u64, u64), PathBuf> = HashMap::new();

	// And it'll all go into a Metadata collection
	let mut md = Metadata::default();

	// Missing we already got, just type convert it
	md.dashes = HashSet::from_iter(missing.into_iter());

	// OK, now just go over 'em one by one.  Since .into_iter() takes
	// ownership, it disassembles the Vec as we go, so we shouldn't be
	// making copies of the data, or repeatedly popping off the front.
	use crate::metadata::{MetaFile, MetaHardLink, MetaDir, MetaSymLink};
	for f in oks.into_iter()
	{
		// First things first; if there's >1 link, it might be a
		// hardlink.  So check if it's in our list; if it is, then we
		// don't have much to do.  And if it's not, we need to mark that
		// we've seen it.
		if f.nlink > 1
		{
			// Have we seen it?
			let din = (f.dev, f.ino);
			if let Some(targ) = hlinks.get(&din)
			{
				// Yep, it is.
				let path = f.path;
				let target = targ.to_path_buf();
				let mhl = MetaHardLink { path, target };
				md.hardlinks.insert(mhl.path.clone(), mhl);
				continue;
			}

			// OK, this is the first; store it up for future dupes and
			// move on.
			hlinks.insert(din, f.path.clone());
		}

		// OK, now we know it's either a File, Dir, or SymLink.  Well...
		// I guess it could actually be something else, but it would have
		// already thrown an error down in the scanner for that.  I'm not
		// sure how we'd deal with it nonfatally, but we'd have to
		// rethink this layer to handle it anyway.
		//
		// Go ahead and pre-extract the conditions into easy names for
		// structuring the results.  Special case: we ignore all flags
		// except schg, because otherwise life is too annoying.
		let uid   = f.uid;
		let gid   = f.gid;
		let mode  = f.mode;
		let flags = f.flags & libc::SF_IMMUTABLE as u32;
		use pool::FileType as FT;
		match f.ftype
		{
			FT::Dir => {
				// Dirs we just have the name and permissions.
				let path = f.path;
				let mdd = MetaDir { path, uid, gid, mode, flags };
				md.dirs.insert(mdd.path.clone(), mdd);
			},
			FT::File => {
				// Files have name, hash, and permissions.  Hash should
				// be guaranteed present.
				let path = f.path;
				let sha256 = match hash {
					true => f.sha256.expect("Must have hash for file"),
					false => Default::default(), // standin value
				};
				let mdf = MetaFile { path, sha256, uid, gid, mode, flags };
				md.files.insert(mdf.path.clone(), mdf);
			},
			FT::SymLink => {
				// And symlinks are really just the name and target,
				// though we _are_ tracking perms too.
				let path = f.path;
				let target = f.symlink.expect("Must have symlink for symlink");
				let mds = MetaSymLink { path, target, uid, gid, mode, flags };
				md.symlinks.insert(mds.path.clone(), mds);
			},
		}
	}



	// And that's it
	Ok(md)
}



/// Scan a set of paths to find all the files with the schg flag set.
/// This gets used in the install process to find what we might need to
/// unset the flags on.
pub(crate) fn schg(basedir: PathBuf, paths: Vec<PathBuf>)
		-> Result<Vec<(PathBuf, u32)>, anyhow::Error>
{
	// Let our scanning pool do the walking
	use crate::core::pool::scan as pool;
	let ctrl = pool::Control { basedir, hash: false, ..Default::default() };
	let sp = pool::Scan::new(paths.len());

	// Send it
	let scanres = {
		use crate::core::pool::Pool as _;
		sp.run(&ctrl, paths)?
	};

	// Split it up for easier access.
	let pool::PoolResult { oks, missing: _, errs } = scanres;

	// If there were any _error_ errors, we should just expect to fail.
	if let Some(errs) = errs { return Err(errs)?; }

	// We only care about going through the Ok's, and finding which ones
	// have the flag set.
	let schg = libc::SF_IMMUTABLE;
	let mut ret = Vec::new();
	for f in oks
	{
		if (f.flags as u64 & schg) != 0
		{
			ret.push((f.path, f.flags));
		}
	}

	Ok(ret)
}
