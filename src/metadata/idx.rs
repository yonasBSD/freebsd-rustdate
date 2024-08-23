//! Metadata index handling
use std::path::{Path, PathBuf};
use std::collections::HashSet;

use crate::util::hash::Sha256Hash;


/// The contents of the metadata index fetched from the server; this is
/// basically just a bunch of hashes of the actual metadata files.
#[derive(Debug, Default, Clone, PartialEq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct MetadataIdx
{
	// INDEX-{ALL,NEW,OLD} are apparently the things we always get here.
	//
	// The sh script also talks about an INDEX-PRESENT, but it only seems
	// to be a thing that it builds locally for local use, it's not a
	// thing that exists on the server.  So as far as the files that are
	// in the server metadata thing, that we fetch and hash and validate
	// and whatnot, these 3 are it.
	//
	// -ALL seems to be used only in the upgrade and IDS commands, while
	// fetch only uses -NEW/-OLD.
	hash_all: Option<Sha256Hash>,
	hash_new: Option<Sha256Hash>,
	hash_old: Option<Sha256Hash>,
}

impl MetadataIdx
{
	/// Parse out a byte blob into ourself
	pub(crate) fn parse(buf: &[u8]) 
			-> Result<MetadataIdx, anyhow::Error>
	{
		parse_metadataidx(buf)
	}

	/// Get named bits out of the struct; this is useful for calls where
	/// we want to look at some pieces, without hardcoding a lot of
	/// interior knowledge up there, or exterior knowledge down here.
	fn get<'a>(&'a self, which: &str) -> Option<&'a Sha256Hash>
	{
		match which {
			"all" => self.hash_all.as_ref(),
			"new" => self.hash_new.as_ref(),
			"old" => self.hash_old.as_ref(),
			_     => None,
		}
	}

	/// Get a [sub]set of the hashes
	pub(crate) fn get_matching<'a, T: AsRef<str>>(&'a self, which: &[T])
			-> Vec<&'a Sha256Hash>
	{
		// The range of possible number of returns is tiny, so just alloc
		// the max up front.
		let mut ret = Vec::with_capacity(Self::alltypes().len());
		which.iter().for_each(|t| {
			self.get(t.as_ref()).and_then(|h| { ret.push(h); Some(()) });
		});

		ret
	}

	/// Make a copy with a [sub]set of the entries
	pub(crate) fn clone_matching<T: AsRef<str>>(&self, which: &[T])
			-> Self
	{
		let mut new = Self::default();
		for f in which
		{
			match f.as_ref()
			{
				"all" => new.hash_all = self.hash_all.clone(),
				"new" => new.hash_new = self.hash_new.clone(),
				"old" => new.hash_old = self.hash_old.clone(),
				_     => panic!("Invalid which: {}", f.as_ref()),
			}
		}

		new
	}

	fn alltypes() -> &'static [&'static str]
	{
		&["all", "new", "old"]
	}


	/// Which of our metadata files don't exist in a given output dir?
	pub(crate) fn not_in_dir(&self, dir: &Path, which:&[impl AsRef<str>])
			-> HashSet<String>
	{
		let mut missing = HashSet::new();

		let check_file_hash = |h: &str| -> bool {
			dir.join(h).is_file()
		};

		// As with f-u.sh, our goal here is "get the <which> entries from
		// our current metadata ($TINDEXHASH)".  Higher levels also do
		// the "and entries from previous runs' metadata (tINDEX.present)
		// if they differ" by doing this in another &self.
		self.get_matching(which).iter().for_each(|h| {
			let gzfile = format!("{}.gz", h);
			if !check_file_hash(&gzfile) { missing.insert(gzfile); }
		});

		missing
	}


	/// Check the hashes of metadata files.  This decompresses 'em out of
	/// one dir into another; generally, this will be files/xyz.gz ->
	/// tmp/xyz.  Of course, we don't need to _save_ the output to check
	/// the hash, but we're gonna load the data a little later anyway, so
	/// "waste" a little temporary space to save decompressing multiple
	/// times, like the sh does.
	///
	/// Returns Ok or the list of files with mismatched sums
	pub(crate) fn check_hashes(&self, fromdir: &Path, todir: &Path,
			which:&[impl AsRef<str>])
			-> Result<(), Vec<String>>
	{
		// Get the list of .gz filenames
		let mut mdfiles = Vec::with_capacity(which.len());
		self.get_matching(which).iter().for_each(|h| {
			let gzfile = format!("{}.gz", h);
			mdfiles.push(gzfile);
		});

		// Iter over 'em and accumulate any wrong'uns
		let mut efiles = Vec::new();
		for f in mdfiles.iter()
		{
			// The basename is the SHA256, which makes it simple.
			// We'll decompress and cache the decompressed files in
			// our tmpdir.
			let hash = f.trim_end_matches(".gz");

			macro_rules! err { () => { efiles.push(f); } }

			// Get the decompressed file
			use crate::util::compress;
			let dret = compress::decompress_gz_dirs(fromdir, todir, f);
			let pf = match dret {
				Ok(file) => file,
				Err(_) => { err!(); continue; },
			};

			// Shouldn't be possible
			if !pf.is_file() { err!(); continue; }

			// OK, now we can check its hash
			use crate::util::hash;
			match hash::check_sha256_file(&pf, hash) {
				Ok(()) => (),
				Err(_) => { err!(); continue; },
			};
		}


		// If it was all OK, then we're done
		if efiles.len() == 0 { return Ok(()); }

		// If not, come up with at least mildly useful errors.
		let eret = efiles.into_iter().map(|p| {
				let path = fromdir.join(p);
				let pdis = path.display();
				if path.exists()
				{
					std::fs::remove_file(&path).unwrap();
					format!("{pdis}: mismatched checksum, deleting.")
				}
				else { format!("{pdis}: missing") }
			}).collect();
		Err(eret)
	}


	/// Build the name in the tempdir of a given metadata file.  Users of
	/// this are assumed to already know the file is there (or not care
	/// if it's there, anyway).
	pub(crate) fn one_tmpfile(&self, dir: &Path, which: &str)
			-> Option<PathBuf>
	{
		let mdhash = self.get(which)?;
		Some(dir.join(mdhash.to_buf().as_ref()))
	}


	/// Parse out a [sub]set of the metadata files.
	pub(crate) fn parse_one(&self, dir: &Path, which: &str)
			-> Result<super::MetadataGroup, Vec<super::ParseFileErr>>
	{
		let mdfile = match self.one_tmpfile(dir, which) {
			Some(mdf) => mdf,
			None => {
				// Programmer error?  A given mdidx that doesn't have all
				// the entries that we expect for a given command should
				// have been caught earlier?  We'll just call this s
				// NotFound IO error I guess...
				use std::io::{Error, ErrorKind as EK};
				let d_s = which.to_string();
				let ioe = Error::new(EK::NotFound, d_s);
				return Err(vec![ioe.into()]);
			}
		};

		super::parse::file(&mdfile)
	}


	/// Handy frontend: parse out a single metadata file from this index,
	/// and do the common alterations to its contents.
	pub(crate) fn parse_one_full(&self, which: &str,
			dir: &Path, config: &crate::config::Config)
			-> Result<super::MetadataGroup, anyhow::Error>
	{
		let mut mdg = match self.parse_one(dir, which) {
			Ok(m) => m,
			Err(e) => {
				println!("");
				eprintln!("Errors parsing {}:", which);
				e.iter().for_each(|e| eprintln!("  {}", e));
				anyhow::bail!("Invalid metadata file, bailing.");
			},
		};

		// Make the various alterations to the contents of the metadata
		// we generally want to do.
		mdg.keep_components(&config.components);
		mdg.remove_paths_matching(&config.ignore_paths);
		mdg.rewrite_kern_dirs()?;

		// And there it is.
		Ok(mdg)
	}
}

// XXX It seems like sometimes we might have additional metadata files to
// work with?  Unclear ATM...


// Notes on what the .sh is doing, 'cuz I keep having to re-figure.  In
// the 'fetch' case, in fetch_metadata():
//
// - Fetch metadata file, which has INDEX-{ALL,NEW,OLD}
// - Get the NEW/OLD lines out of that
// - Combine those with anything else from tINDEX.present (cache from
//   previous runs?) into tINDEX.new.
// - (metadata patchfiles; revisit)
// - Fetch all the (missing) hashfiles from .new
// - Sanity check the .new
// - Remove hashfiles from .present that aren't in .new
// - Cleanup downloaded metadata file
// - Stash our .new over to .present for next run



// Do the parsing of the index file out into the structure
fn parse_metadataidx(buf: &[u8])
		-> Result<MetadataIdx, anyhow::Error>
{
	let mut idb = MetadataIdx::default();
	let idlines = buf.split(|c| *c == b'\n');
	for line in idlines
	{
		if line.len() < 1 { continue }
		let mut spl = line.split(|c| *c == b'|');
		let (name, hash) = (spl.next(), spl.next());

		let name = match name {
			Some(x) => x,
			None => continue,
		};
		let hash = match hash {
			Some(x) => x,
			None => continue,
		};

		use std::str;
		let hashify = |h: &[u8]|
			-> Result<Sha256Hash, anyhow::Error> {
				Ok(str::from_utf8(h)?.parse()?)
			};
		match name
		{
			b"INDEX-ALL" => idb.hash_all = Some(hashify(hash)?),
			b"INDEX-NEW" => idb.hash_new = Some(hashify(hash)?),
			b"INDEX-OLD" => idb.hash_old = Some(hashify(hash)?),
			_ => continue,
		}
	}

	Ok(idb)
}




#[cfg(test)]
pub(super) mod tests
{
	use super::parse_metadataidx;
	use super::MetadataIdx;

	// Builders for test bits
	fn mk_midx_bits() -> MetadataIdx
	{
		let mut it = super::MetadataIdx::default();
		it.hash_all = Some("4fa4fde15d81a117ec13cf7758717f75f982bfb3c54a9fb6d1da61e928e43288"
				.parse().unwrap());
		it.hash_new = Some("67932f69c954a4f89389ee54b34f7feca8e104dba1c2d0aeef8f9871cdf02c26"
				.parse().unwrap());
		it.hash_old = Some("ce46b8868d86aecb5f44d4f9c84b241c5007e28c13d2521db590b9be36c60491"
				.parse().unwrap());
		it
	}

	fn mk_midx(midx: &MetadataIdx) -> String
	{
		format!("INDEX-ALL|{}\nINDEX-NEW|{}\nINDEX-OLD|{}\n",
				midx.hash_all.as_ref().unwrap(), midx.hash_new.as_ref().unwrap(),
				midx.hash_old.as_ref().unwrap())
	}

	// Just to be sure that did what I expected...
	const MDIDX: &str = r##"
INDEX-ALL|4fa4fde15d81a117ec13cf7758717f75f982bfb3c54a9fb6d1da61e928e43288
INDEX-NEW|67932f69c954a4f89389ee54b34f7feca8e104dba1c2d0aeef8f9871cdf02c26
INDEX-OLD|ce46b8868d86aecb5f44d4f9c84b241c5007e28c13d2521db590b9be36c60491
"##;

	#[test]
	fn mk_midx_check()
	{
		let midx = mk_midx_bits();
		let mstr = mk_midx(&midx);
		assert_eq!(mstr.trim(), MDIDX.trim());
	}

	#[test]
	fn good_parse()
	{
		// Good parse should be... y'know.  Good.
		let idx = parse_metadataidx(MDIDX.as_bytes()).unwrap();
		assert_eq!(idx, mk_midx_bits());
	}
}
