//! Hash checking pool
//!
//! The goal here is to check the hashes of a bunch of downloaded files
//! (usually in tmpdir), and if they're good, save them off into another
//! location (usually filesdir).
use std::path::PathBuf;

use crate::util::hash;

use indicatif::ProgressBar;



/// An impl of the threadpool for hash checking
#[derive(Debug)]
pub(crate) struct HashCheck
{
	/// We'll kick a progress bar
	pb: ProgressBar,

	/// Sucessful results
	oks: Vec<Res>,

	/// Errors
	errs: Vec<HashCheckErr>,
}

impl HashCheck
{
	pub(crate) fn new(pblen: usize) -> Self
	{
		Self {
			pb: indicatif::ProgressBar::new(pblen.try_into().unwrap()),
			oks:  Vec::new(),
			errs: Vec::new(),
		}
	}
}


/// The final cumulative result of a scan run
#[derive(Debug)]
pub(crate) struct PoolResult
{
	/// Successful scan results
	pub(crate) oks: Vec<Res>,

	/// Errors
	pub(crate) errs: Option<PoolErrs>,
}

/// Errors found in scanning
#[derive(Debug)]
#[derive(thiserror::Error)]
#[error("{errs:?}")]  // kinda fugly...
pub(crate) struct PoolErrs
{
	/// Some number of individual errors
	pub(crate) errs: Vec<HashCheckErr>,
}

/// Control for scanning; we're under a basedir
#[derive(Debug, Clone)]
pub(crate) struct Control
{
	/// The dir where the temporary files to check are; usually our
	/// tempdir.
	pub(crate) tmpdir: PathBuf,

	/// The final location to store the good hashes; usually our
	/// filesdir.
	pub(crate) filesdir: PathBuf,

	/// Whether to keep the uncompressed copy in the tmpdir.  If we're
	/// likely to be reusing this file later in our processing, that
	/// saves the re-decompression.
	pub(crate) keep: bool,
}

/// A single work request
#[derive(Debug)]
pub(crate) struct Req
{
	/// The filename to check.  Expecting <hash>.gz; the expected hash
	/// will be derived from that.
	pub(crate) path: String,
}

/// The result of a single check
#[derive(Debug)]
pub(crate) struct Res
{
	/// Yeup, this was was OK.  The output file <hash> was correct, and
	/// <filesdir>/<hash>.gz is made.
	pub(crate) hash: String,
}

/// Error in the checking
#[derive(Debug)]
#[derive(thiserror::Error)]
pub(crate) enum HashCheckErr
{
	/// Missing file
	#[error("No such file: {0}")]
	Missing(PathBuf),

	/// Filesystem IO error of some kind.
	#[error("I/O error: {0}")]
	Io(#[from] std::io::Error),

	/// Compression error.
	#[error("Compression error: {0}")]
	Compress(anyhow::Error),

	/// Hashing error
	#[error("Hashing error: {0}")]
	Hashing(#[from] hash::Sha256ReaderErr),
}



/// Now connect all those bits in
impl crate::core::pool::Pool for HashCheck
{
	// Individual control types just have our copy of basedir in them.
	type Control = Control;
	type UnitControl = Control;

	// For the per-thread copy, we'll just clone
	fn mk_unitcontrol(c: &Control) -> Control { c.clone() }

	// The final returned results
	type PoolResult = PoolResult;


	// The individual work items and their results
	type WorkRequest = Req;
	type WorkResult  = Res;
	type WorkErr     = HashCheckErr;
	fn work(ctrl: &Control, req: Req) -> Result<Res, HashCheckErr>
	{
		hashcheck_worker(ctrl, req)
	}


	// This is a CPU job
	fn nthreads(&self) -> u32 { super::jobs_cpu() }


	// Processing the result of a stash
	fn work_result(&mut self, resp: Result<Res, HashCheckErr>)
	{
		// Well, we did a thing, so kick our progress
		self.pb.inc(1);

		// Did it succeeed?  Then accumulate up the info.  Did it fail?
		// Accumulate up the fails.
		match resp
		{
			Ok(r)  => self.oks.push(r),
			Err(e) => self.errs.push(e),
		}
	}


	// Finalize up our in-progress tracking to our final result
	fn finalize(self) -> PoolResult
	{
		// Split ourselves up
		let HashCheck { pb, oks, errs } = self;

		// The progress bar is done
		pb.finish();

		// Setup an errs we got
		let errs = match errs.len() {
			0 => None,
			_ => Some(PoolErrs { errs }),
		};

		// And build the return
		let ret = PoolResult { oks, errs };
		ret
	}
}




/// HashCheckning a single file
fn hashcheck_worker(ctrl: &Control, req: Req)
		-> Result<Res, HashCheckErr>
{
	use HashCheckErr as SE;
	use std::fs;
	use crate::util::path_join;
	use crate::util::compress;

	// What the hash we'll be expecting is
	let hashstr = &req.path.trim_end_matches(".gz");

	// The input file to look at.  In theory; sometimes, this gets called
	// with the source .gz in tmpdir, and sometimes with the decompressed
	// version already there.
	let srcpath = path_join(&ctrl.tmpdir, &req.path);

	// The final location
	let dstpath = path_join(&ctrl.filesdir, &req.path);

	// Decompress it into the same dir; this will give us our
	// decompressed path too.  This silently succeeds if the decompressed file
	// is already there, which sometimes it is.
	let decpath = compress::decompress_gz_dirs(&ctrl.tmpdir, &ctrl.tmpdir,
			&req.path)
			.map_err(|e| SE::Compress(e))?;

	// OK, well, it's there now, right?  Shouldn't be fallible...
	if !decpath.is_file() { Err(SE::Missing(srcpath.to_path_buf()))?; }


	// Check the hash
	hash::check_sha256_file(&decpath, hashstr)?;

	// OK, it was good, move it into the final location.  Though if the
	// compressed version wasn't already there, we need to make it.
	match srcpath.is_file() {
		true => fs::rename(&srcpath, &dstpath)?,
		false => {
			compress::compress_gz(&decpath, &srcpath)?;
			fs::rename(&srcpath, &dstpath)?;
		},
	};

	// And maybe remove the decompressed file.
	if !ctrl.keep { fs::remove_file(&decpath)?; }

	// Well OK then
	let mut hash = req.path;
	let gz = ".gz";
	if hash.ends_with(gz) { hash.truncate(hash.len() - gz.len()); }
	let res = Res { hash };
	Ok(res)
}
