//! Metadata struct handlers
use std::path::{Path, PathBuf};
use std::collections::{HashSet, HashMap};

use super::Metadata;
use super::MetaChange;
use super::MetadataLine;

use regex_lite::Regex;


impl Metadata
{
	/// Minor util: do we have no entries of any kind?
	pub(crate) fn empty(&self) -> bool
	{
		if self.dirs.len() > 0      { return false; }
		if self.files.len() > 0     { return false; }
		if self.symlinks.len() > 0  { return false; }
		if self.hardlinks.len() > 0 { return false; }
		if self.dashes.len() > 0    { return false; }
		true
	}


	/// How many entries (of all kinds)?
	pub(crate) fn len(&self) -> usize
	{
		self.dirs.len()
			+ self.files.len()
			+ self.symlinks.len()
			+ self.hardlinks.len()
			+ self.dashes.len()
	}


	/// Get a list of all the pathnames that anything in this Metadata
	/// deals with.  We presume no overlap, since there shouuldn't be.
	pub(crate) fn allpaths(&self) -> Vec<&Path>
	{
		let paths = self.allpaths_hashset();
		paths.into_iter().collect()
	}


	/// Get all the pathnames that anything in this Metadata deals with.
	/// We presume no overlap, since there shouldn't be.
	pub(crate) fn allpaths_hashset(&self) -> HashSet<&Path>
	{
		self.allpaths_hashset_inner(true)
	}

	/// Get all the pathnames that anything in this Metadata deals with,
	/// with the exception of dash lines (since those indicate "not
	/// really a thing").
	pub(crate) fn allpaths_hashset_nodash(&self) -> HashSet<&Path>
	{
		self.allpaths_hashset_inner(false)
	}

	// Inner impl of building up the paths
	fn allpaths_hashset_inner(&self, dashes: bool) -> HashSet<&Path>
	{
		let mut cap = self.dirs.len()
				+ self.files.len()
				+ self.symlinks.len()
				+ self.hardlinks.len()
				;
		if dashes { cap += self.dashes.len(); }
		let mut ret = HashSet::with_capacity(cap);

		ret.extend(self.dirs.keys().map(|p| p.as_path()));
		ret.extend(self.files.keys().map(|p| p.as_path()));
		ret.extend(self.symlinks.keys().map(|p| p.as_path()));
		ret.extend(self.hardlinks.keys().map(|p| p.as_path()));
		if dashes
		{ ret.extend(self.dashes.iter().map(|p| p.as_path())); }

		ret
	}


	/// Get whatever entry we might have for a given path, no matter what
	/// type it's for.
	pub(crate) fn get_path(&self, p: &Path) -> Option<MetadataLine>
	{
		macro_rules! doone {
			($fld: ident) => {
				if let Some(m) = self.$fld.get(p)
				{
					return Some(m.clone().into());
				}
			};
		}

		doone!(files);
		doone!(dirs);
		doone!(symlinks);
		doone!(hardlinks);

		// Dashes don't store the otherwise-pointless struct
		if self.dashes.contains(p)
		{
			let path = p.to_path_buf();
			let md = super::MetaDash { path };
			return Some(md.into());
		}

		None
	}


	/// Get entries for a set of paths (consuming the paths).
	pub(crate) fn get_from_paths(&self, mut paths: Vec<PathBuf>)
			-> HashMap<PathBuf, MetadataLine>
	{
		let mut mds = HashMap::with_capacity(paths.len());

		for p in paths.drain(..)
		{
			let mdl = match self.get_path(&p) {
				Some(l) => l,
				None    => continue,
			};
			mds.insert(p, mdl);
		}

		mds
	}


	// /// Turn a Metadata into a bit map of lines.  This is roughly the
	// /// same as self.get_from_paths([ ... all the paths ...]), except
	// /// we're consuming the input.
	// pub(crate) fn into_lines(self) -> HashMap<PathBuf, MetadataLine>
	// {
	// 	let mut ret = HashMap::with_capacity(self.len());
    //
	// 	self.files.into_iter().for_each(|(p, m)| { ret.insert(p, m.into()); });
	// 	self.dirs.into_iter().for_each(|(p, m)| { ret.insert(p, m.into()); });
	// 	self.symlinks.into_iter().for_each(|(p, m)|
	// 			{ ret.insert(p, m.into()); });
	// 	self.hardlinks.into_iter().for_each(|(p, m)|
	// 			{ ret.insert(p, m.into()); });
    //
	// 	self.dashes.into_iter().for_each(|p| {
	// 		let dl = MetaDash { path: p.clone() };
	// 		ret.insert(p, dl.into());
	// 	});
    //
	// 	ret
	// }


	/// Turn ourself into a SplitTypes for install-like operations.
	pub(crate) fn into_split_types(self) -> super::SplitTypes
	{
		// From impl handles it
		self.into()
	}


	/// Build a copy with paths filtered by regexps.  This is a
	/// convenience wrapper over clone() + filter_paths_regexps().
	///
	/// This is expensive in an absolute sense, since you're .clone()'ing
	/// something then removing bits. But compared to a .sh script
	/// spitting out intermediate files, I feel no urgent need to try and
	/// be more efficient.  Probably the logic for the places using this
	/// could be implemented in a more idiomatic way, but it's enough
	/// trouble dissecting what the sh is trying to accomplish.  May
	/// revisit this when we get more done.
	///
	/// This method's consumers all seem to use it as a temporary, so
	/// this could probably be impl'd with a reference to a subset of
	/// Self, but that's tricky enough to setup, and the expense of doing
	/// this is so in the noise, that I'm not bothering to try at the
	/// moment.
	pub(crate) fn with_filter_paths_regexps(&self, re: &[Regex]) -> Self
	{
		let mut ret = self.clone();
		ret.filter_paths_regexps(re);
		ret
	}


	/// Retain paths matching a set of regexes.
	pub(crate) fn filter_paths_regexps(&mut self, re: &[Regex])
	{
		let matches = |p: &PathBuf| -> bool {
			let f = p.to_string_lossy();
			re.iter().any(|r| r.is_match(&f))
		};

		self.dirs.retain(      |k, _v| matches(k));
		self.files.retain(     |k, _v| matches(k));
		self.symlinks.retain(  |k, _v| matches(k));
		self.hardlinks.retain( |k, _v| matches(k));
		self.dashes.retain(    |k|     matches(k));
	}


	/// Build an output of any entries with differing metadata
	/// (owner/mode/flags).
	///
	/// This is used in impl'ing the equivalent of f-u.sh's
	/// fetch_filter_modified_metadata(), but the .sh will trigger on
	/// different _types_ too.  We don't, and it's hardly clear what
	/// should actually be done in that case anyway.
	pub(crate) fn modified_metadata(&self, other: &Self) -> Self
	{
		let mut ret = Self::default();

		// Only file/symlink/dir have metadata
		macro_rules! comp {
			($fld:ident) => {
				for (ak, a) in &self.$fld
				{
					let b = match other.$fld.get(ak) {
						Some(b) => b,
						None    => continue,
					};
					// Use cmp_ugid here so that, when we're ignoring
					// ownership (like when running as non-root), we
					// don't count that as a change.  Otherwise we might
					// miss a really-changed mode from old->new, because
					// we considered the my-owned stuff as being a
					// ModifiedMetadata to Keep.
					use super::cmp_ugid;
					if false
							|| !cmp_ugid(&a.uid, &b.uid)
							|| !cmp_ugid(&a.gid, &b.gid)
							|| a.mode  != b.mode
							|| a.flags != b.flags
					{
						ret.$fld.insert(ak.clone(), a.clone());
					}
				}
			};
		}
		comp!(files);
		comp!(dirs);
		comp!(symlinks);

		ret
	}


	/// Copy metadata from another MD.
	pub(crate) fn replace_metadata_from(&mut self, other: &Self)
	{
		// Only file/symlink/dir have metadata
		macro_rules! copy {
			($fld:ident) => {
				for (ak, a) in &mut self.$fld
				{
					let b = match other.$fld.get(ak) {
						Some(b) => b,
						None    => continue,
					};
					a.uid   = b.uid;
					a.gid   = b.gid;
					a.mode  = b.mode;
					a.flags = b.flags;
				}
			};
		}
		copy!(files);
		copy!(dirs);
		copy!(symlinks);
	}


	/// Remove all the entries that match another MD.  That is, keep all
	/// the {files, dirs, etc} in self that aren't the same as what's in
	/// other.
	pub(crate) fn remove_matching(&mut self, other: &Self)
	{
		self.remove_matching_inner(other, true)
	}

	/// Remove all the entries that match another MD (check-sys version).
	/// This differes from the more normal operation in that we're
	/// showing changes, which means that hardlinks that target the same
	/// file aren't changed (if the target file is changed, that's
	/// something we'll be talking about, but the hardlink isn't in this
	/// usage).
	pub(crate) fn remove_matching_checksys(&mut self, other: &Self)
	{
		self.remove_matching_inner(other, false)
	}

	/// remove_matching inner impl
	fn remove_matching_inner(&mut self, other: &Self, spechl: bool)
	{
		self.files.retain(|k, a|     { Some(&*a) != other.files.get(k) });
		self.dirs.retain(|k, a|      { Some(&*a) != other.dirs.get(k) });
		self.symlinks.retain(|k, a|  { Some(&*a) != other.symlinks.get(k) });

		// Hardlinks are sometimes a special case.  In most usage, if we
		// have any hardlinks that refer to a file that's still in in
		// myself after the above, it means the target file has changed,
		// which means that even if the target of the hardlink is the
		// same, that file will be replaced, so the hardlink will need
		// remaking.
		//
		// Not all usages need this handling though, so we have a
		// condition guarding it.
		self.hardlinks.retain(|k, a| {
			if spechl && self.files.contains_key(&a.target) { return true; }
			Some(&*a) != other.hardlinks.get(k)
		});

		// It's unclear that the dash lines ever really matter where
		// we're using this, but for completeness...
		self.dashes.retain(|k| !other.dashes.contains(k));
	}


	/// Find all the matching entries between two MD's.  This is used to
	/// do things like remove_matching() above, but when we need to be a
	/// bit broader about what/how we do it.
	///
	/// We're collapsing down the matched paths just to a single list,
	/// rather than separate lists for each type, because where we would
	/// use this we care about the paths, not so much their type, because
	/// of how they'll be used.
	pub(crate) fn find_matching(&self, other: &Self) -> HashSet<PathBuf>
	{
		let mut ret = HashSet::new();

		self.files.iter().for_each(|(k, a)| {
			if Some(a) == other.files.get(k) { ret.insert(k.to_path_buf()); }
		});
		self.dirs.iter().for_each(|(k, a)| {
			if Some(a) == other.dirs.get(k) { ret.insert(k.to_path_buf()); }
		});
		self.symlinks.iter().for_each(|(k, a)| {
			if Some(a) == other.symlinks.get(k) { ret.insert(k.to_path_buf()); }
		});

		// As in remove_matching(), a hardlink matching means not only
		// the target is the same, but the file it's pointing at is the
		// same.
		self.hardlinks.iter().for_each(|(k, a)| {
			let mut matched = true;
			if Some(a) != other.hardlinks.get(k) { matched = false; }
			if matched
			{
				let mf = self.files.get(&a.target);
				let of = other.files.get(&a.target);
				if mf != of { matched = false; }
			}
			if matched { ret.insert(k.to_path_buf()); }
		});

		// It's unclear that the dash lines ever really matter where
		// we're using this, but for completeness...
		self.dashes.iter().for_each(|k| {
			if other.dashes.contains(k) { ret.insert(k.to_path_buf()); }
		});

		ret
	}


	/// Remove all the entries matching some set of paths.  This is
	/// generally intended to be used with find_matching(), to implement
	/// a more general variant of what remove_matching() does.
	pub(crate) fn remove_paths(&mut self, paths: &HashSet<PathBuf>)
	{
		self.files.retain(|k, _v| { !paths.contains(k) });
		self.dirs.retain(|k, _v| { !paths.contains(k) });
		self.symlinks.retain(|k, _v| { !paths.contains(k) });
		self.hardlinks.retain(|k, _v| { !paths.contains(k) });

		// x-ref above on dashes
		self.dashes.retain(|k| !paths.contains(k));
	}


	/// Keep [only] the entries matching some set of paths.
	pub(crate) fn keep_paths(&mut self, paths: &HashSet<&Path>)
	{
		self.files.retain(|k, _v|     { paths.contains(k.as_path()) });
		self.dirs.retain(|k, _v|      { paths.contains(k.as_path()) });
		self.symlinks.retain(|k, _v|  { paths.contains(k.as_path()) });
		self.hardlinks.retain(|k, _v| { paths.contains(k.as_path()) });

		// x-ref above on dashes
		self.dashes.retain(|k| paths.contains(k.as_path()));
	}


	/// Remove entries with the path matching some set of regexps.
	pub(crate) fn remove_paths_matching(&mut self, paths: &[Regex])
	{
		let matching = |p: &Path| -> bool {
			let pstr = p.to_string_lossy();
			paths.iter().any(|r| r.is_match(&pstr))
		};

		self.files.retain(|k, _v|     { !matching(k) });
		self.dirs.retain(|k, _v|      { !matching(k) });
		self.symlinks.retain(|k, _v|  { !matching(k) });
		self.hardlinks.retain(|k, _v| { !matching(k) });
		self.dashes.retain(|k|        { !matching(k) });
	}


	/// Keep [only] the entries matching some set of paths.
	pub(crate) fn keep_paths_matching(&mut self, paths: &[Regex])
	{
		let matching = |p: &Path| -> bool {
			let pstr = p.to_string_lossy();
			paths.iter().any(|r| r.is_match(&pstr))
		};

		self.files.retain(|k, _v|     { matching(k) });
		self.dirs.retain(|k, _v|      { matching(k) });
		self.symlinks.retain(|k, _v|  { matching(k) });
		self.hardlinks.retain(|k, _v| { matching(k) });
		self.dashes.retain(|k|        { matching(k) });
	}


	/// Absorb another Metadata.
	///
	/// Because this uses HashMap::extend(), keys from other will
	/// override our existing keys.  For the moment, that's OK, because
	/// the only place we're using it is one where we know there's no
	/// overlap anyway.
	pub(crate) fn extend(&mut self, other: Self)
	{
		self.files.extend(other.files);
		self.dirs.extend(other.dirs);
		self.symlinks.extend(other.symlinks);
		self.hardlinks.extend(other.hardlinks);
		self.dashes.extend(other.dashes);
	}


	/// Return info about any Paths changing type between two MetaData's.
	/// e.g., files turning into directories, etc.
	///
	/// Mostly, this would be used for the cur-vs-new case, where a
	/// current file turns into a directory in the new version or the
	/// like.
	pub(crate) fn type_changes(&self, other: &Self)
			-> HashMap<PathBuf, MetaChange>
	{
		let mut ret = HashMap::new();

		// We'll be a little inefficient about how we do this, in order
		// to be a little cleaner.  So start out by making a copy and
		// doing something like remove_matching(), except just removing
		// things that are the same type.
		//
		// I think we'll just ignore the crap outta the Dash lines, they
		// seem underspecified and probably don't matter here.
		let mut cur = self.clone();
		cur.files.retain(     |k, _v| !other.files.contains_key(k));
		cur.dirs.retain(      |k, _v| !other.dirs.contains_key(k));
		cur.symlinks.retain(  |k, _v| !other.symlinks.contains_key(k));
		cur.hardlinks.retain( |k, _v| !other.hardlinks.contains_key(k));

		// First off, files turning into hardlinks or hardlinks turning
		// into files seems like a sorta nullity, for the purposes of
		// this "changing type", since it's kinda a wonky corner of
		// things.  So dump them too.
		cur.files.retain(     |k, _v| !other.hardlinks.contains_key(k));
		cur.hardlinks.retain( |k, _v| !other.files.contains_key(k));


		// Abstract the handling
		macro_rules! comp {
			($path: ident, $cent:ident, $fld:ident) => {
				if let Some(o) = other.$fld.get($path)
				{
					let old = $cent.clone().into();
					let new = o.clone().into();
					let mc = MetaChange { old, new };
					ret.insert($path.clone(), mc);
					continue;
				}
			};
		}

		// Files may turn into dirs or symlinks
		for (p, curf) in &cur.files
		{
			// Turning into a dir?  That seems to have happened several
			// times with c++ includes.
			comp!(p, curf, dirs);

			// Turning into a symlink?  /etc/motd at some point I think.
			comp!(p, curf, symlinks);
		}

		// Hardlinks I guess we should treat the same as file
		for (p, curf) in &cur.hardlinks
		{
			comp!(p, curf, dirs);
			comp!(p, curf, symlinks);
		}

		// Dirs?
		for (p, curd) in &cur.dirs
		{
			// Turning into a file or symlink, that makes sense
			comp!(p, curd, files);
			comp!(p, curd, symlinks);

			// Turning into a hardlink is...  like turning into a file I
			// guess?
			comp!(p, curd, hardlinks);
		}

		// And symlinks mostly look like dirs
		for (p, curs) in &cur.symlinks
		{
			comp!(p, curs, files);
			comp!(p, curs, dirs);
			comp!(p, curs, hardlinks);
		}


		ret
	}
}




#[cfg(test)]
mod tests
{
	use std::path::PathBuf;

	use crate::metadata::Metadata;

	#[test]
	fn modified_metadata()
	{
		use crate::metadata::MetaFile;

		let mut old = Metadata::default();
		let mut new = Metadata::default();
		let mut cur = Metadata::default();

		let fname: PathBuf = "/foo/bar".into();
		let mut xfile = MetaFile::default();
		xfile.path = fname.clone();
		xfile.uid = 1;
		xfile.gid = 2;
		xfile.mode = 0o751;
		xfile.flags = 0o4000;

		// Put it in the same state in old/new
		old.files.insert(fname.clone(), xfile.clone());
		new.files.insert(fname.clone(), xfile.clone());

		// Give it different mode in cur
		let mut cfile = xfile.clone();
		cfile.mode = 0o755;
		cur.files.insert(fname.clone(), cfile.clone());

		// Double check our setup
		assert_eq!(old.files[&fname], new.files[&fname]);
		assert_ne!(old.files[&fname], cur.files[&fname]);

		// Actually, just check the whole things, [Partial]Eq should work
		// like that, right?
		assert_eq!(old, new);
		assert_ne!(old, cur);

		// Set some sample hashes to know they're untouched in the copying
		// over.
		let ohash = [1u8; 32].into();
		let nhash = [2u8; 32].into();
		let chash = [3u8; 32].into();
		old.files.get_mut(&fname).unwrap().sha256 = ohash;
		new.files.get_mut(&fname).unwrap().sha256 = nhash;
		cur.files.get_mut(&fname).unwrap().sha256 = chash;

		// Should get nothing if we try to make a modified from old->new.
		let nomods = old.modified_metadata(&new);
		assert_eq!(nomods.empty(), true);

		// But old->cur we should
		let mods = old.modified_metadata(&cur);
		assert_eq!(mods.empty(), false);

		// And once we copy it in, cur/new should look the same, but old,
		// not so much.  mode changes, the rest not
		new.replace_metadata_from(&cur);

		assert_eq!(old.files[&fname].uid,   new.files[&fname].uid);
		assert_eq!(old.files[&fname].gid,   new.files[&fname].gid);
		assert_ne!(old.files[&fname].mode,  new.files[&fname].mode);
		assert_eq!(old.files[&fname].flags, new.files[&fname].flags);

		assert_eq!(cur.files[&fname].uid,   new.files[&fname].uid);
		assert_eq!(cur.files[&fname].gid,   new.files[&fname].gid);
		assert_eq!(cur.files[&fname].mode,  new.files[&fname].mode);
		assert_eq!(cur.files[&fname].flags, new.files[&fname].flags);

		// But the hashes are all still different
		assert_ne!(old.files[&fname].sha256, new.files[&fname].sha256);
		assert_ne!(old.files[&fname].sha256, cur.files[&fname].sha256);
		assert_ne!(cur.files[&fname].sha256, new.files[&fname].sha256);
	}


	#[test]
	fn remove_matching()
	{
		use crate::metadata::MetaFile;

		let mut old = Metadata::default();
		let mut new = Metadata::default();

		let fname: PathBuf = "/foo/bar".into();
		let mut xfile = MetaFile::default();
		xfile.path = fname.clone();
		xfile.uid = 1;
		xfile.gid = 2;
		xfile.mode = 0o751;
		xfile.flags = 0o4000;

		// Put it in the same state in old/new
		old.files.insert(fname.clone(), xfile.clone());
		new.files.insert(fname.clone(), xfile.clone());

		// Now, if we remove matching from old, it should wind up empty.
		let mut nold = old.clone();
		nold.remove_matching(&new);
		assert_eq!(nold.empty(), true);

		// But if we change something, it shouldn't
		let mut nold = old.clone();
		nold.files.get_mut(&fname).unwrap().mode = 0o755;
		nold.remove_matching(&new);
		assert_eq!(nold.empty(), false);
		assert!(nold.files.get(&fname).is_some());
	}


	#[test]
	fn find_remove_paths()
	{
		use crate::metadata::{MetaFile, MetaDir};

		let mut old = Metadata::default();
		let mut new = Metadata::default();

		let fname: PathBuf = "/foo/bar".into();
		let mut xfile = MetaFile::default();
		xfile.path = fname.clone();
		xfile.uid = 1;
		xfile.gid = 2;
		xfile.mode = 0o751;
		xfile.flags = 0o4000;

		// Put it in the same state in old/new
		old.files.insert(fname.clone(), xfile.clone());
		new.files.insert(fname.clone(), xfile.clone());

		// Pull that matching list, which should have the 1 entry.
		let matches = old.find_matching(&new);
		assert_eq!(matches.len(), 1, "1 matching entry");

		// Now move that name off into dirs
		let mut xdir = MetaDir::default();
		xdir.path = fname.clone();
		xdir.uid = 1;
		xdir.gid = 2;
		xdir.mode = 0o751;
		xdir.flags = 0o0;
		new.files.remove(&fname);
		new.dirs.insert(fname.clone(), xdir);

		// Should be there.
		assert!(new.dirs.get(&fname).is_some());

		// But after we remove the matchers...
		new.remove_paths(&matches);

		// not so much anymore
		assert!(new.dirs.get(&fname).is_none());
	}


	#[test]
	fn find_remove_path_regex()
	{
		let pfoo = PathBuf::from("/foo");
		let pbar = PathBuf::from("/bar");

		let mut md = Metadata::default();
		md.dashes.insert(pfoo.clone());
		md.dashes.insert(pbar.clone());

		use regex_lite::Regex;
		let fo_re  = Regex::new(r"/fo").unwrap();
		let baz_re = Regex::new(r"/baz").unwrap();
		let both_re = &[fo_re, baz_re];

		// Try keeping just the /fo's
		let mut md1 = md.clone();
		assert_eq!(md1.len(), 2, "Starts with 2");
		md1.keep_paths_matching(both_re);
		assert_eq!(md1.len(), 1, "Ended with 1");
		assert!(md1.dashes.contains(&pfoo), "Kept foo");
		assert!(!md1.dashes.contains(&pbar), "Lost bar");

		// And removing
		let mut md1 = md.clone();
		assert_eq!(md1.len(), 2, "Starts with 2");
		md1.remove_paths_matching(both_re);
		assert_eq!(md1.len(), 1, "Ended with 1");
		assert!(!md1.dashes.contains(&pfoo), "Lost foo");
		assert!(md1.dashes.contains(&pbar), "Kept bar");
	}
}
