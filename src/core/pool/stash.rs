//! File stashing pool
use std::path::PathBuf;

use crate::util::hash;

use indicatif::ProgressBar;



/// An impl of the threadpool for scanning
#[derive(Debug)]
pub(crate) struct Stash
{
	/// We'll kick a progress bar
	pb: ProgressBar,

	/// Sucessful results
	oks: Vec<Res>,

	/// Errors
	errs: Vec<StashErr>,
}

impl Stash
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
	pub(crate) errs: Vec<StashErr>,
}

/// Control for scanning; we're under a basedir
#[derive(Debug, Clone)]
pub(crate) struct Control
{
	/// The basedir where source files live under
	pub(crate) basedir: PathBuf,

	/// The dir we store the hashfiles into
	pub(crate) filesdir: PathBuf,

	/// The tempdir we use in the process
	pub(crate) tmpdir: PathBuf,
}

/// A single work request
#[derive(Debug)]
pub(crate) struct Req
{
	/// For a given file
	pub(crate) path: PathBuf,

	/// Expecting a particular hash
	pub(crate) hash: hash::Sha256HashBuf,
}

/// The result of a single stash
#[derive(Debug)]
pub(crate) struct Res
{
	/// The file we knocked out
	#[allow(dead_code)]
	pub(crate) path: PathBuf,
}

/// Error in stashing
#[derive(Debug)]
#[derive(thiserror::Error)]
pub(crate) enum StashErr
{
	/// Not a file.  Sorta an Io, but it means it's some other type,
	/// which means we lost a race since our scan.
	#[error("Not a file")]
	BadType(PathBuf),

	/// Filesystem IO error of some kind.  Compression errors wind up
	/// here.
	#[error("File I/O error: {0}")]
	Io(#[from] std::io::Error),

	/// Hashing error
	#[error("Hashing error: {0}")]
	Hashing(#[from] hash::Sha256ReaderErr),

	/// Hash didn't match; this indicates a race, since we already stored
	/// up the hash in our scanning just moments ago
	#[error("Hash didn't match: expected {0}, got {1}")]
	HashMismatch(String, String),

	/// Some other misc thing that doesn't fit that.
	#[error("Internal error: {0}")]
	Misc(anyhow::Error),
}



/// Now connect all those bits in
impl crate::core::pool::Pool for Stash
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
	type WorkErr     = StashErr;
	fn work(ctrl: &Control, req: Req) -> Result<Res, StashErr>
	{
		stash_worker(ctrl, req)
	}


	// This is a CPU job
	fn nthreads(&self) -> u32 { super::jobs_cpu() }


	// Processing the result of a stash
	fn work_result(&mut self, resp: Result<Res, StashErr>)
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
		let Stash { pb, oks, errs } = self;

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




/// Stashning a single file
fn stash_worker(ctrl: &Control, req: Req) -> Result<Res, StashErr>
{
	use StashErr as SE;
	use std::fs;

	// Where's the actual file to check?
	use crate::util::path_join;
	let srcpath = path_join(&ctrl.basedir, &req.path);

	// Temp location
	let hstr = req.hash.as_ref();
	let tmppath = &ctrl.tmpdir.join(hstr);

	// Final location
	let hgzstr = format!("{}.gz", hstr);
	let finalpath = &ctrl.filesdir.join(&hgzstr);


	// Trivial check.  Of course, this is racy too, but it's already a
	// tiny race since the scan, and if you're messing with system files
	// while you're running a system upgrade util, you deserve to get
	// grumpy errors.
	if !srcpath.is_file() { Err(SE::BadType(srcpath.to_path_buf()))?; }

	// Copy it
	fs::copy(&srcpath, tmppath)?;

	// Check the hash
	use hash::check_sha256_file;
	if let Err(e) = check_sha256_file(&tmppath, hstr)
	{
		use hash::Sha256ReaderErr as HE;
		return Err(match e {
			HE::Hash(exp, got) => SE::HashMismatch(exp, got),
			HE::IO(e) => SE::Io(e),
			HE::Expected(s) => SE::Misc(s),
		})?;
	}

	// Compress it (still in the temp loc, just to be crash safe-ish).
	use crate::util::compress;
	let tmpgz = ctrl.tmpdir.join(&hgzstr);
	compress::compress_gz(&srcpath, &tmpgz)?;

	// And move over to final
	fs::rename(&tmpgz, &finalpath)?;

	// And we're done
	let res = Res { path: req.path };
	Ok(res)
}
