//! bspatch'ing pool
//!
//! Here we just take an input file, patch it into an output, and mention
//! what we made.  We expect a list of input hashes and patch names,
//! where the patch names look like "<inputhash>-<outputhash>", so we
//! will try to patch the file "<tmpdir>/<inputhash>" into the file
//! "<tmpdir>/<outputhash>".  If there's no input file, we'll try
//! decompressing it from "<filesdir>/<inputhash>.gz" first.
//!
//! We don't attempt to validate either of the hashes here; this step
//! will be followed up by validating the hashes of the output files and
//! compressing them into <filesdir> in a separate hashcheck run anyway,
//! so it doesn't much matter if something is wrong in this step.
use std::path::PathBuf;

use indicatif::ProgressBar;



/// An impl of the threadpool for hash checking
#[derive(Debug)]
pub(crate) struct Patch
{
	/// We'll kick a progress bar
	pb: ProgressBar,

	/// Sucessful results
	oks: Vec<Res>,

	/// Errors
	errs: Vec<PatchErr>,
}

impl Patch
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


/// The final cumulative result of a patch run
#[derive(Debug)]
pub(crate) struct PoolResult
{
	/// Successfully patched files
	pub(crate) oks: Vec<Res>,

	/// Errors
	pub(crate) errs: Option<PoolErrs>,
}

/// Patching errors
#[derive(Debug)]
#[derive(thiserror::Error)]
#[error("{errs:?}")]  // kinda fugly...
pub(crate) struct PoolErrs
{
	/// Some number of individual errors
	pub(crate) errs: Vec<PatchErr>,
}

/// Control for patching; where files wind up.
///
/// It's intentional that this has all the fields needed to fill up a
/// pool::hashcheck::Control, so we can easily make one from the other if
/// needed.
#[derive(Debug, Clone)]
pub(crate) struct Control
{
	/// The dir where the temporary files to check are; usually our
	/// tempdir.  This is where we expect <inputhash> and <patchfile>,
	/// and where we'll write <outputhash>.
	pub(crate) tmpdir: PathBuf,

	/// The long-term files dir.  We'll look here for <inputhash>.gz if
	/// we're lacking <tmpdir>/<inputhash> and decompress it over.
	pub(crate) filesdir: PathBuf,

	/// Whether to keep the uncompressed copy in the tmpdir.  If we're
	/// likely to be reusing this file later in our processing, that
	/// saves the re-decompression.
	pub(crate) keep: bool,
}

impl From<Control> for super::hashcheck::Control
{
	fn from(c: Control) -> Self
	{
		let Control {tmpdir, filesdir, keep} = c;
		Self {tmpdir, filesdir, keep}
	}
}


/// A single work request
#[derive(Debug)]
pub(crate) struct Req
{
	/// The name of the patch file; it'll look something like
	/// "<inhash>-<outhash>".  We'll derive the input and output files
	/// from it.
	pub(crate) patch: String,
}

/// The result of a single patching
#[derive(Debug)]
pub(crate) struct Res
{
	/// Succeeded: name of the output file.  Will presumable be <outhash>
	/// (and will be in <tmpdir>, but we'll just name the hash).
	pub(crate) hash: String,
}

/// Error in the patching
#[derive(Debug)]
#[derive(thiserror::Error)]
pub(crate) enum PatchErr
{
	/// Invalid patch name.  This probably indicates the programmer
	/// screwed up.
	#[error("Invalid patch name: '{0}'")]
	BadPatch(String),

	/// Can't find input <inhas> file.
	#[error("No input file: '{0}'")]
	Missing(String),

	/// Some sort of IO error.  This could be our filemaking, or
	/// patching; they both get here.
	#[error("I/O error: {0}")]
	IO(#[from] std::io::Error),

	/// Compression error.
	#[error("Compression error: {0}")]
	Compress(anyhow::Error),
}



/// Now connect all those bits in
impl crate::core::pool::Pool for Patch
{
	type Control = Control;
	type UnitControl = Control;

	// For the per-thread copy, we'll just clone
	fn mk_unitcontrol(c: &Control) -> Control { c.clone() }

	// The final returned results
	type PoolResult = PoolResult;


	// The individual work items and their results
	type WorkRequest = Req;
	type WorkResult  = Res;
	type WorkErr     = PatchErr;
	fn work(ctrl: &Control, req: Req) -> Result<Res, PatchErr>
	{
		patch_worker(ctrl, req)
	}


	// This is a CPU job
	fn nthreads(&self) -> u32 { super::jobs_cpu() }


	// Processing the result
	fn work_result(&mut self, resp: Result<Res, PatchErr>)
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
		let Patch { pb, oks, errs } = self;

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




/// Patch'ing a single file
fn patch_worker(ctrl: &Control, req: Req)
		-> Result<Res, PatchErr>
{
	use PatchErr as PE;
	use crate::util::path_join;
	use crate::util::compress;
	use crate::util::bspatch;

	// First, split up the patch name into the input and output hashes
	let patch = req.patch;
	let mut inout: Vec<&str> = patch.splitn(2, '-').collect();
	if inout.len() != 2 { return Err(PE::BadPatch(patch)); }
	let outhash = inout.pop().ok_or_else(|| PE::BadPatch(patch.clone()))?;
	let inhash  = inout.pop().ok_or_else(|| PE::BadPatch(patch.clone()))?;
	if !inout.is_empty() { return Err(PE::BadPatch(patch)); }
	let _ = inout;

	// Now, where's the source file...
	let srcpath = path_join(&ctrl.tmpdir, inhash);
	if !srcpath.is_file()
	{
		// Try decompressing it out of filesdir
		let compfile = format!("{inhash}.gz");
		match compress::decompress_gz_dirs(&ctrl.filesdir, &ctrl.tmpdir,
				&compfile)
		{
			Ok(_) => (),
			Err(e) => {
				// Separate out "didn't find file" from any other IO
				// error.  Now that it's going through anyhow for the
				// context, we need to downcast to check that...
				if let Some(ioe) = e.downcast_ref::<std::io::Error>()
				{
					use std::io::ErrorKind as EK;
					match ioe.kind() {
						EK::NotFound => Err(PE::Missing(inhash.to_string()))?,
						_ => (),
					}
				}

				// Otherwise it's some other compression error
				Err(PE::Compress(e))?;
			},
		}
	}
	if !srcpath.is_file()
	{
		// Shouldn't be possible at this point, but...
		return Err(PE::Missing(inhash.to_string()))?;
	}

	// OK, we have the source file.  In theory, we could do some tests on
	// the patchfile and output file here, but I'm going to just assume
	// presence.  We _should_ know the patchfiles exists, or it wouldn't
	// have been fed into us, and if we can't write into the outfile,
	// we'll just transfer up the IO error.  So from here, we just build
	// the paths and pass them to our patcher.
	let dstpath = path_join(&ctrl.tmpdir, outhash);
	let patchpath = path_join(&ctrl.tmpdir, &patch);
	bspatch::patch(&srcpath, &dstpath, &patchpath)?;

	// Success!  Maybe cleanup, and return.
	if !ctrl.keep { std::fs::remove_file(&srcpath)?; }
	let ret = Res { hash: outhash.to_string() };
	Ok(ret)
}
