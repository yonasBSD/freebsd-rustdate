//! FS scanner pool impl
use std::path::PathBuf;

use crate::util;
use util::hash::Sha256Hash;

use indicatif::ProgressBar;



/// An impl of the threadpool for scanning
#[derive(Debug)]
pub(crate) struct Scan
{
	/// We'll kick a progress bar
	pb: ProgressBar,

	/// Sucessful scan results
	oks: Vec<Res>,

	/// Missing files
	missings: Vec<PathBuf>,

	/// Other scan errors
	errs: Vec<ScanErr>,
}

impl Scan
{
	pub(crate) fn new(pblen: usize) -> Self
	{
		Self {
			pb: indicatif::ProgressBar::new(pblen.try_into().unwrap()),
			oks:  Vec::new(),
			errs: Vec::new(),
			missings: Vec::new(),
		}
	}
}


/// The final cumulative result of a scan run
#[derive(Debug)]
pub(crate) struct PoolResult
{
	/// Successful scan results
	pub(crate) oks: Vec<Res>,

	/// Missing files
	pub(crate) missing: Vec<PathBuf>,

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
	pub(crate) errs: Vec<ScanErr>,
}

/// Control for scanning; we're under a basedir
#[derive(Debug, Clone)]
#[derive(derivative::Derivative)]
#[derivative(Default)]
pub(crate) struct Control
{
	/// The basedir we're searching under
	pub(crate) basedir: PathBuf,

	/// Whether to hash files
	#[derivative(Default(value="true"))]
	pub(crate) hash: bool,
}

/// The result of a single file scan
#[derive(Debug)]
pub(crate) struct Res
{
	/// The scanned file
	pub(crate) path: PathBuf,

	/// What type we found
	pub(crate) ftype: FileType,

	/// SHA256 hash of the contents.  Only meaningful for regular files,
	/// not dirs or symlinks or the like.
	pub(crate) sha256: Option<Sha256Hash>,

	/// Symlinks would have a target
	pub(crate) symlink: Option<PathBuf>,

	// Misc FS metadata
	pub(crate) dev:   u64,
	pub(crate) ino:   u64,
	pub(crate) nlink: u64,
	pub(crate) uid:   u32,
	pub(crate) gid:   u32,
	pub(crate) mode:  u32,
	pub(crate) flags: u32,
}

/// Internal helper for the type of a file
#[derive(Debug)]
pub(crate) enum FileType { File, Dir, SymLink }
impl TryFrom<std::fs::FileType> for FileType
{
	type Error = std::fs::FileType;

	fn try_from(ft: std::fs::FileType) -> Result<Self, Self::Error>
	{
		if ft.is_file()    { return Ok(Self::File); }
		if ft.is_dir()     { return Ok(Self::Dir); }
		if ft.is_symlink() { return Ok(Self::SymLink); }
		Err(ft)
	}
}
impl TryFrom<u16> for FileType
{
	type Error = u16;

	fn try_from(st_mode: u16) -> Result<Self, Self::Error>
	{
		use libc::{S_IFMT, S_IFDIR, S_IFREG, S_IFLNK};
		let ft = st_mode & S_IFMT;
		match ft {
			S_IFREG => Ok(Self::File),
			S_IFDIR => Ok(Self::Dir),
			S_IFLNK => Ok(Self::SymLink),
			_ => Err(ft)
		}
	}
}

/// Error in scanning a path
#[derive(Debug)]
#[derive(thiserror::Error)]
pub(crate) enum ScanErr
{
	/// No such file.  Technically this would be Io too, but for our
	/// purposes, this is a normal-ish expected thing to be handled;
	/// other I/O errors mostly mean "WTF, something's broken"...
	#[error("No such file")]
	Nonexistent(PathBuf),

	/// Filesystem IO error of some kind
	#[error("File I/O error: {0}")]
	Io(#[from] std::io::Error),

	/// Some other misc thing that doesn't fit that.
	#[error("Internal error: {0}")]
	Misc(String),
}

impl From<util::LstatErr> for ScanErr
{
	fn from(e: util::LstatErr) -> Self
	{
		use util::LstatErr as LE;
		match e {
			LE::CString(e)     => Self::Misc(format!("CString error: {e}")),
			LE::Nonexistent(p) => Self::Nonexistent(p),
			LE::Lstat(_, estr) => Self::Misc(format!("lstat(2): {estr}")),
		}
	}
}



/// Now connect all those bits in
impl crate::core::pool::Pool for Scan
{
	// Individual control types just have our copy of basedir in them.
	type Control = Control;
	type UnitControl = Control;

	// For the per-thread copy, we'll just clone
	fn mk_unitcontrol(c: &Control) -> Control { c.clone() }

	// The final returned results
	type PoolResult = PoolResult;


	// The individual work items and their results
	type WorkRequest = PathBuf;
	type WorkResult  = Res;
	type WorkErr     = ScanErr;
	fn work(ctrl: &Control, req: PathBuf) -> Result<Res, ScanErr>
	{
		scan_worker(ctrl, req)
	}


	// This is a CPU job
	fn nthreads(&self) -> u32 { super::jobs_cpu() }


	// Processing the result of a single scan
	fn work_result(&mut self, resp: Result<Res, ScanErr>)
	{
		// Well, we did a thing, so kick our progress
		self.pb.inc(1);

		// Did it succeeed?  Then accumulate up the info.  Did it fail?
		// Assumulate up the fails.
		match resp
		{
			Ok(r)  => self.oks.push(r),
			Err(e) => {
				// This could be a "file not found", which isn't
				// an "error", just more info.  Or it could be
				// some other kinda error, which _is_ an error.
				use ScanErr as SE;
				match e
				{
					SE::Nonexistent(p) => self.missings.push(p),
					e => self.errs.push(e),
				}
			},
		}
	}


	// Finalize up our in-progress tracking to our final result
	fn finalize(self) -> PoolResult
	{
		// Split ourselves up
		let Scan { pb, oks, missings, errs } = self;

		// The progress bar is done
		pb.finish();

		// Setup an errs we got
		let errs = match errs.len() {
			0 => None,
			_ => Some(PoolErrs { errs }),
		};

		// And build the struct
		let missing = missings;
		let ret = PoolResult { oks, missing, errs };
		ret
	}
}




/// Scanning a single file
fn scan_worker(ctrl: &Control, path: PathBuf) -> Result<Res, ScanErr>
{
	use ScanErr as SE;

	// Where's the actual file to check?
	use crate::util::path_join;
	let realpath = path_join(&ctrl.basedir, &path);

	// Most of the info we care about is in the extended metadata stuff.
	// In fact, almost all of it.  Except the flags.  Apparently we have
	// to use the libc crate for that...
	let ftype;
	let dev;
	let ino;
	let nlink;
	let uid;
	let gid;
	let mode;
	let flags;
	if false
	{
		use std::os::unix::fs::MetadataExt;
		let md = std::fs::symlink_metadata(&realpath)
				.map_err(|e| {
					// "Not found" is an "error", but we treat it separately
					// from other harder errors.
					use std::io::ErrorKind as EK;
					match e.kind()
					{
						EK::NotFound => SE::Nonexistent(path.clone()),
						_ => e.into(),
					}
				})?;

		ftype = md.file_type().try_into().map_err(|e| {
				SE::Misc(format!("Unknown type for {realpath:?}: {e:?}"))
			})?;
		dev = md.dev();
		ino = md.ino();
		nlink = md.nlink();
		uid = md.uid();
		gid = md.gid();
		mode = md.mode() & 0o7777; // Only care about u/g/o and the setXid's

		// I mean, I s'pose we could just do all the libc::stat stuff
		// here to get at this, and at least have all the "normal" bits
		// above dealt with, right?
		flags = 0;  // Dangit...
	}
	else
	{
		// OK, we've wrapped up the grimiest of these details...
		//
		// Note we need to manually mess with the Nonexistent return
		// case, since it'll have the full path (including basedir), and
		// that'll mess us up.
		let (myst, _) = util::lstat(&realpath)
				.map_err(|e| {
					use util::LstatErr::Nonexistent as NE;
					match e {
						NE(_) => NE(path.clone()),
						e => e
					}
				})?;

		// If we got this far, it's successful, so pull out the bits we
		// want.
		ftype = myst.mode.try_into().map_err(|e| {
				let estr = format!("Unknown mode filetype for {}: {}",
						realpath.to_string_lossy(), e);
				SE::Misc(estr)
			})?;
		dev   = myst.dev;
		ino   = myst.ino;
		nlink = myst.nlink;
		uid   = myst.uid;
		gid   = myst.gid;
		mode  = myst.perms.into();
		flags = myst.flags;
	}


	// OK, if it's a regular file, calculate the sha256 of it.  If it's a
	// symlink, we need to see what it's pointing to.
	let mut sha256 = None;
	let mut symlink = None;
	match ftype
	{
		FileType::File => {
			if ctrl.hash
			{
				use crate::util::hash;
				let fh = match hash::sha256_file(&realpath) {
					Ok(h) => h,
					Err(e) => {
						use hash::Sha256ReaderErr as HE;
						let ne = match e {
							HE::IO(e) => e.into(),
							x => SE::Misc(x.to_string()),
						};
						return Err(ne);
					},
				};
				sha256 = Some(fh);
			}
		},
		FileType::SymLink => {
			let sl = std::fs::read_link(&realpath)?;
			symlink = Some(sl);
		},
		_ => (),
	}


	// OK, if we made it here, we succeeded at stating a thing.
	let res = Res { path, ftype, symlink, sha256,
			dev, ino, nlink, uid, gid, mode, flags };
	Ok(res)
}
