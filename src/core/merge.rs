//! File merging bits.
//!
//! In principal, there could be value in making a Pool for this.  In
//! practice, it's hard to imagine more than maybe a few dozen of these
//! coming up, in files no bigger than a few dozen k, so just doing it
//! single-threaded should already be ridiculously fast.
use std::path::{Path, PathBuf};
use std::fs;

/// Files we don't bother trying to merge.
static DONT_MERGE_STRS: &[&str]  = &[
	// passwd stuff; this will all be regen'd from master.passwd
	// anyway.
	"/etc/passwd",
	"/etc/spwd.db",
	"/etc/pwd.db",

	// Other .db files generated from /etc files
	"/etc/login.conf.db",

	// XXX This probably wouldn't have wound up in MergeChanges
	// anyway?  That feels a bit like a bug...
	"/var/db/services.db",
];

/// Don't merge particular Path's
pub(crate) fn dont_merge() -> Vec<PathBuf>
{
	let pbuf = |p: &&str| -> PathBuf {
		<&str as AsRef<Path>>::as_ref(p).to_path_buf()
	};
	DONT_MERGE_STRS.iter().map(|p| pbuf(p)).collect()
}



/*
 * Aggregated info about the results of merges.  This is stuff that will
 * get stuffed into our persistent state to handle the commands
 * displaying or resolving merge issues, etc.
 *
 * These are generally expected to be put into a HashMap<PathBuf,
 * [self]>, not be standalone, so I'm not including paths in them, just
 * the metainfo.
 */
use crate::util::hash::Sha256HashBuf;

/// Successful (clean) merges.  Referenced files should all be in
/// `<filesdir>/<hash>.gz`.
#[derive(Debug)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Clean
{
	/// The 'old' file hash; the presumed merge base.
	pub(crate) old: Sha256HashBuf,

	/// The 'new' file hash; the new upstream version
	pub(crate) new: Sha256HashBuf,

	/// The 'current' file hash; the file on the running system.
	pub(crate) cur: Sha256HashBuf,

	/// The resulting file's hash; should be stored in <filesdir>.
	pub(crate) res: Sha256HashBuf,
}


/// Conflicted merge.  Referenced file should be in `<filesdir>/<hash>.gz`.
#[derive(Debug)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Conflict
{
	/// The 'old' file hash; the presumed merge base.
	pub(crate) old: Sha256HashBuf,

	/// The 'new' file hash; the new upstream version
	pub(crate) new: Sha256HashBuf,

	/// The 'current' file hash; the file on the running system.
	pub(crate) cur: Sha256HashBuf,

	/// The resulting file with diff3 conflict markers in it
	pub(crate) res: Sha256HashBuf,
}






/*
 * Performing merges
 */

/// Merge errors
#[derive(Debug)]
#[derive(thiserror::Error)]
pub(crate) enum MergeError
{
	/// Merge had conflicts
	#[error("Conflicts found in merge")]
	Conflicts,

	/// Some other I/O error
	#[error("I/O error: {0}")]
	IO(#[from] std::io::Error),
}


/// Attempt to merge files into an output file.
///
/// Err(Conflicts) is "expected" to happen with some regularity, and is a
/// signal to pass up to the user to resolve.  IO errors are probably
/// something fatal.
pub(crate) fn merge_files(old: &[u8], cur: &[u8], new: &[u8],
		out: &mut fs::File) -> Result<(), MergeError>
{
	// diffy only works on in-memory stuff.  It has separate functions
	// for merging &str's and &[u8]'s, but inspection of the source
	// doesn't suggest there's any actual _gain_ from working on str's,
	// so don't bother trying to str-ify the files.
	use diffy::merge_bytes;

	// merge_bytes() returns the Vec<u8> of the merge results, but in Ok
	// for success and Err for conflicts.  So extract it out, and define
	// our return.
	let ret;
	use MergeError::Conflicts as EC;
	let mbytes = match merge_bytes(&old, &cur, &new) {
		Ok(b)  => { ret = Ok(());  b },
		Err(b) => { ret = Err(EC); b },
	};

	// Write out and return
	use std::io::Write as _;
	out.write_all(&mbytes)?;
	ret
}



/*
 * Creating diffs.
 *
 * Yeah, this isn't really 'merge', but we mostly only do it for
 * displaying to the user, so...
 */

/// Show the diff for the result of a 'clean' merge.
///
/// This is intended for human reading, not necessarily for direct
/// application.
pub(crate) fn merge_diff(fname: &Path, src: &[u8], dst: &[u8]) -> Vec<u8>
{
	use diffy::create_patch_bytes;

	let patch = create_patch_bytes(src, dst);
	let mut pbytes = patch.to_bytes();

	// XXX diffy doesn't seem to have a useful way to let us _set_ the
	// filenames in the friggin' patch, so we kinda fake it...
	let defhdr = b"--- original\n+++ modified\n";
	let dhlen = defhdr.len();
	match pbytes.get(0..dhlen)
	{
		None => (),
		Some(s) => {
			// Replace with our naming
			let file = fname.display();
			let nhdr = format!("--- {file}  (original)\n\
					+++ {file}  (modified)\n").as_bytes().to_vec();
			pbytes.splice(0..s.len(), nhdr);
		},
	};

	pbytes
}
