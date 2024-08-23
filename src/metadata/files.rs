//! Stuff related to handling files in a Metadata
use std::path::{Path, PathBuf};
use std::collections::HashMap;

use super::{Metadata, MetaFile};
use crate::util::hash;


impl Metadata
{
	/// Get a list of all the files in a metadata that don't have
	/// <hash>.gz's in a dir.  This is mostly used to stash up current
	/// files.
	pub(crate) fn files_no_hash_dir(&self, hdir: &Path)
			-> Option<Vec<&Path>>
	{
		let missing = self.files_hashdir_compare_be(hdir)?;

		let ret = missing.into_iter().map(|mf| mf.path.as_ref()).collect();
		Some(ret)
	}


	/// Get a list of all the hashes of files in a metadata that don't
	/// have <hash>.gz's in a dir.  This is useful in answering "what do
	/// we need to get" questions.
	pub(crate) fn hashes_no_hash_dir(&self, hdir: &Path)
			-> Option<Vec<hash::Sha256HashBuf>>
	{
		let missing = self.files_hashdir_compare_be(hdir)?;

		// We'll go ahead and do the Hash -> HashBuf change.  Our callers
		// will be wanting the hex strings anyway, so we could just make
		// String's of them, but the buf is probably enough, and saves
		// some indirection.
		let mut ret: Vec<_> = missing.into_iter()
				.map(|mf| { mf.sha256.to_buf() })
				.collect();

		// Can get duplicated hashes tho, since multiple files can have
		// the same hash.
		ret.sort_unstable();
		ret.dedup();

		Some(ret)
	}


	/// Get a list of the MetaFile's in a Metadata that don't have
	/// <hash>.gz's in a dir.  This is used for doing various checks of
	/// things we expect to need against our <filesdir> stash.
	fn files_hashdir_compare_be(&self, hdir: &Path) -> Option<Vec<&MetaFile>>
	{
		let mut ret = Vec::new();

		self.files.iter().for_each(|(_fpath, mf)| {
			let hgz = format!("{}.gz", mf.sha256);
			if !hdir.join(hgz).is_file() { ret.push(mf) }
		});

		match ret.len() {
			0 => None,
			_ => Some(ret)
		}
	}


	/// Stash up a set of files in a hashdir.  We're given a set of paths
	/// that are (presumptively) part of our .files member, and a dir to
	/// stash into.  This sticks the files into <filehash>.gz in that
	/// dir.  This is used to store up unmodified copies and rollback
	/// data.
	pub(crate) fn stash_files(&self, files: &[&Path],
			basedir: PathBuf, tmpdir: PathBuf, filesdir: PathBuf)
			-> Result<usize, anyhow::Error>
	{
		use crate::core::pool::stash as pool;

		// Prep up all the scan requests.  We have to be a little smart,
		// because there _do_ exist identical files (that aren't
		// hardlinks), so we can get duplicated hashes, which means we
		// could get collisions in the process.
		let mut hseen = std::collections::HashSet::new();
		let reqs: Vec<pool::Req> = files.iter().filter_map(|p| {
			let path = p.to_path_buf();
			let hash = self.files[&path].sha256.to_buf();
			if hseen.contains(&hash) { return None; }
			hseen.insert(hash.clone());
			Some(pool::Req { path, hash })
		}).collect();

		// We build a threadpool do to this
		let ctrl = pool::Control { basedir, filesdir, tmpdir };
		let sp = pool::Stash::new(files.len());

		// And run it
		let stres = {
			use crate::core::pool::Pool as _;
			sp.run(&ctrl, reqs)?
		};

		// See what we got
		let pool::PoolResult { oks, errs } = stres;

		// No errors (presumptively normal) means everything's peachy.
		// Errors means not so peachy
		match errs {
			Some(e) => Err(e)?,
			None    => Ok(oks.len()),
		}
	}



	/// Build up a list of the MetaFiles between two Metadata's that have
	/// the same hashes.  This is used in figuring out patches to
	/// download, since the metadata doesn't matter (so just a == between
	/// the MetaFile's is unhelpful).
	pub(crate) fn intersect_files_hash(&self, other: &Self)
			-> Option<HashMap<&Path, &MetaFile>>
	{
		let mut ret = HashMap::new();

		self.files.iter().for_each(|(fpath, mf)| {
			let omf = match other.files.get(fpath) {
				Some(f) => f,
				None => return,
			};
			if mf.sha256 == omf.sha256 { ret.insert(fpath.as_ref(), mf); }
		});

		match ret.len() {
			0 => None,
			_ => Some(ret)
		}
	}
}
