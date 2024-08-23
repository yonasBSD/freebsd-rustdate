//! Filesystem-related funcs.
//!
//! This is mostly just some wrappers over low-level stuff we need that
//! isn't available in std.

use std::path::{PathBuf, Path};
use std::ffi::{self, CString};



/*
 * Some higher-level wrappers
 */

/// Remove schg flag from a file.
///
/// This is mostly only used in the install process for previously-found
/// +schg files, so we already know the flags we expect and can just
/// twiddle it down.
///
/// Not making very detailed errors here; if this fails it's probably
/// catastrophic and loud, so...
pub(crate) fn unschg_file(file: &Path, flags: u32)
		-> Result<(), anyhow::Error>
{
	let schg = libc::SF_IMMUTABLE;
	flag_rm(file, schg, Some(flags.into()))?;
	Ok(())
}


// /// Set flag(s) on a file (if it isn't already)
// pub(crate) fn flag_add(file: &Path, flag: u64, curflag: Option<u64>)
// 		-> Result<u64, anyhow::Error>
// {
// 	flag_twiddle_inner(file, flag, true, curflag)
// }

/// Unset flag(s) on a file (if it's there).
///
/// Returns the (presumptive) new flags on success, or some sorta
/// complaint on error.
pub(crate) fn flag_rm(file: &Path, flag: u64, curflag: Option<u64>)
		-> Result<u64, anyhow::Error>
{
	flag_twiddle_inner(file, flag, false, curflag)
}

fn flag_twiddle_inner(file: &Path, flag: u64, set: bool, curflag: Option<u64>)
		-> Result<u64, anyhow::Error>
{
	// We'll need the CString file
	let fnbytes = file.as_os_str().as_encoded_bytes();
	let f = CString::new(fnbytes)?;

	// Get current flags
	let cur = match curflag {
		Some(f) => f,
		None => {
			let (st, _) = lstat_inner(file, &f)?;
			st.flags.into()
		},
	};

	// Figure the new flags
	let new = match set {
		true  => cur | flag,
		false => cur & !flag,
	};

	// If it's already there, there's nothing to do.
	if cur == new { return Ok(cur) }

	// Else, doit
	lchflags_inner(file, &f, new)?;
	Ok(new)
}



/*
 * Lower-level bits
 */

/// My stat(2) (lstat(2)) return, broken out rustily
#[derive(Debug, Default)]
pub(crate) struct Stat
{
	pub(crate) dev:   u64,
	pub(crate) ino:   u64,
	pub(crate) nlink: u64,
	pub(crate) uid:   u32,
	pub(crate) gid:   u32,
	pub(crate) flags: u32,

	// Raw stat() mode.
	pub(crate) mode:  u16,

	// File permissions
	pub(crate) perms: u16,
}


/// Give some useful-ish erroring
#[derive(Debug)]
#[derive(thiserror::Error)]
#[error("{errs:?}")]  // kinda fugly...
pub(crate) enum LstatErr
{
	/// Couldn't build the filename; should be impossible.
	#[error("CString error: {0}")]
	CString(#[from] ffi::NulError),

	/// File not found
	#[error("File not found: {0}")]
	Nonexistent(PathBuf),

	/// Unknown stat(2) error
	#[error("libc stat(2): error {0}: {1}")]
	Lstat(i32, String),
}


/// lstat(2).  This is a pretty thin wrapper.
///
/// Returns err on failing to find a file to work with.
///
/// On "success", returns our built Stat above, and the raw libc::stat
/// (for uses that go more directly into it).
pub(crate) fn lstat(file: &Path) -> Result<(Stat, libc::stat), LstatErr>
{
	// Make a C-ish string of the filename.
	let fnbytes = file.as_os_str().as_encoded_bytes();
	let f = CString::new(fnbytes)?;

	lstat_inner(file, &f)
}

fn lstat_inner(file: &Path, f: &CString) -> Result<(Stat, libc::stat), LstatErr>
{
	let mut lcst: libc::stat;
	let errno = unsafe {
		use std::mem;

		lcst = mem::zeroed();
		let ret = libc::lstat(f.as_ptr(), &mut lcst);
		let errno: i32 = match ret {
			0 => 0i32,
			_ => *libc::__error(),
		};
		errno
	};

	// errno != 0 means some failure.
	use libc::{ENOENT, ENOTDIR};
	use LstatErr as LE;
	match errno {
		0 => {
			// Success!
			let myst = Stat {
				dev:   lcst.st_dev.into(),
				ino:   lcst.st_ino.into(),
				nlink: lcst.st_nlink.into(),
				uid:   lcst.st_uid,
				gid:   lcst.st_gid,
				flags: lcst.st_flags,
				mode:  lcst.st_mode,
				perms: lcst.st_mode & 0o7777,
			};
			Ok((myst, lcst))
		},
		ENOENT | ENOTDIR => {
			// These are roughly "file not found"-ish, so treat 'em
			// as such.
			Err(LE::Nonexistent(file.to_path_buf()))
		},
		_ => {
			// Anything else, whoTF knows...  this is probably really
			// an Io, but since we're a long way from std::io...
			let estr = unsafe {
				let ce_cchar = libc::strerror(errno);
				let ce_cstr  = ffi::CStr::from_ptr(ce_cchar);
				ce_cstr.to_string_lossy()
			};
			Err(LE::Lstat(errno, estr.into_owned()))
		},
	}
}


/// lchflags(2)
///
/// This mostly just wraps around libc to put us in a more rusty world
/// for our usage.
pub(crate) fn lchflags(file: &Path, flags: u64)
		-> Result<(), anyhow::Error>
{
	// Make a C-ish string of the filename.
	let fnbytes = file.as_os_str().as_encoded_bytes();
	let f = match CString::new(fnbytes) {
		Ok(s) => s,
		Err(e) => anyhow::bail!("CString error: {e}"),
	};

	lchflags_inner(file, &f, flags)
}

fn lchflags_inner(file: &Path, f: &CString, flags: u64)
		-> Result<(), anyhow::Error>
{
	let err = unsafe {
		let ret = libc::lchflags(f.as_ptr(), flags);
		match ret {
			0 => 0i32,
			_ => *libc::__error(),
		}
	};

	match err {
		0 => Ok(()),
		_ => anyhow::bail!("lchflags({}): errno {err}", file.display()),
	}
}
