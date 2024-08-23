//! HTTP fetch pool impl 
use std::path::PathBuf;

use url::Url;



/// The fetching threadpool state
#[derive(Debug)]
pub(crate) struct Fetch
{
	/// We'll kick a progress bar
	pb: indicatif::ProgressBar,

	/// We pre-seeded how many files we expected.  I'm not sure how much
	/// we really need this...
	nfiles: u32,

	/// We'll track the files we got
	okfiles: Vec<String>,

	/// And errors we found
	errs: Vec<GetErr>,
}

impl Fetch
{
	pub(crate) fn new(pblen: usize) -> Self
	{
		Self {
			pb: indicatif::ProgressBar::new(pblen.try_into().unwrap()),
			nfiles: pblen.try_into().unwrap(),
			okfiles: Vec::with_capacity(pblen),  // Assume success
			errs: Vec::new(),
		}
	}
}


/// The final result of a fetching
#[derive(Debug)]
pub(crate) struct PoolResult
{
	/// Attempted files
	pub(crate) nfiles: u32,

	/// Successful files
	pub(crate) okfiles: Vec<String>,

	/// Errors
	pub(crate) errs: Option<PoolErrs>,
}


/// Errors found in a fetching
#[derive(Debug)]
#[derive(thiserror::Error)]
#[error("{errs:?}")]  // kinda fugly...
pub(crate) struct PoolErrs
{
	/// Some number of individual errors
	pub(crate) errs: Vec<GetErr>,
}



/// Control for the fetching pool
#[derive(Debug, Clone)]
pub(crate) struct Control
{
	/// HTTP agent
	pub(crate) agent: ureq::Agent,

	/// Base URL to work from
	pub(crate) baseurl: Url,

	/// Output path to dump the file into
	pub(crate) path: PathBuf,
}

/// A single fetch request
#[derive(Debug)]
pub(crate) struct Req
{
	/// What file to get (from under Control.baseurl, into Control.path)
	pub(crate) file: String,
}

/// A fetch result; just a byte count
#[derive(Debug)]
pub(crate) struct Res
{
	/// The requested file (from the request)
	pub(crate) file: String,

	// /// How many bytes we pulled down
	// pub(crate) bytes: u64,
}

/// A fetch error
#[derive(Debug)]
#[derive(thiserror::Error)]
pub(crate) enum GetErr
{
	/// URL building error
	#[error("URL building error: {0}")]
	Url(#[from] url::ParseError),

	/// HTTP error
	#[error("HTTP fetch error: {0}")]
	Http(#[from] ureq::Error),

	/// Filesystem IO error of some kind
	#[error("File I/O error: {0}")]
	Io(#[from] std::io::Error),
}


// And do the pooling
impl crate::core::pool::Pool for Fetch
{
	// Control has HTTP agents, basedirs, etc
	type Control = Control;
	type UnitControl = Control;

	// For the per-thread copy, just clone
	fn mk_unitcontrol(c: &Control) -> Control { c.clone() }

	// The final returned results
	type PoolResult = PoolResult;


	// The individual work items and their results
	type WorkRequest = Req;
	type WorkResult  = Res;
	type WorkErr     = GetErr;
	fn work(ctrl: &Control, req: Req) -> Result<Res, GetErr>
	{
		scan_worker(ctrl, req)
	}


	// This is a network job
	fn nthreads(&self) -> u32 { super::jobs_net() }


	// Processing the result of a single scan
	fn work_result(&mut self, resp: Result<Res, GetErr>)
	{
		// We did a thing, kick our progress
		self.pb.inc(1);

		// Did it succeeed?  Then we got 1 more.  Fail?  Rack it up.
		match resp
		{
			Ok(r)  => self.okfiles.push(r.file),
			Err(e) => self.errs.push(e),
		}
	}


	// Finalize up our in-progress tracking to our final result
	fn finalize(self) -> PoolResult
	{
		// Split ourselves up
		let Fetch { pb, nfiles, okfiles, errs } = self;

		// The progress bar is done
		pb.finish();

		// If we got errs, we got errs.
		let errs = match errs.len() {
			0 => None,
			_ => Some(PoolErrs { errs }),
		};

		// And build the struct
		let ret = PoolResult { nfiles, okfiles, errs };
		ret
	}
}


/// Doing a single fetch.
fn scan_worker(ctrl: &Control, get: Req) -> Result<Res, GetErr>
{
	use std::{fs, io};

	// Unlike get_bytes(), this is writing out to the filesystem, and
	// expects bigger files than "a little text I'll parse in".
	// Still, let's not make it trivial for a broken or malicious
	// server to fill up our disk, and retain _some_ limit to how big
	// a file can be.  In looking at some servers, I see these going
	// up to high double digit megs, so...  let's say a gig is
	// probably generous?
	const LIMIT: u64 = 1 * 1024 * 1024 * 1024;

	let Req { file } = get;

	// Figure the input URL and output filename
	let inurl = ctrl.baseurl.join(&file)?;
	let outpath = ctrl.path.join(&file);

	// Open up the output file.
	// XXX Maybe imagine tempfiles etc. someday.
	let outfile = fs::File::create(&outpath)?;

	// HTTP responses can come in slow, so may as well wrap this...
	let mut outwrite = io::BufWriter::new(outfile);

	// Make the request
	let agent = &ctrl.agent;
	let resp = match agent.request_url("GET", &inurl).call() {
		Ok(r) => r,
		Err(e) => {
			// Cleanup a bit and bail
			fs::remove_file(&outpath)?;
			return Err(e)?;
		},
	};

	// OK, it worked, take our limit and write it in
	use io::Read;
	let mut rdr = resp.into_reader().take(LIMIT);
	let _bytes = io::copy(&mut rdr, &mut outwrite)?;

	// Goodie
	let outfile = outwrite.into_inner().map_err(|e| e.into_error())?;
	outfile.sync_all()?;

	// let res = Res { file, bytes };
	let res = Res { file };
	Ok(res)
}
