//! Misc util funcs

/// SHA256 hashing utils
pub(crate) mod hash;

/// Compression utils
pub(crate) mod compress;

/// Binary patching
pub(crate) mod bspatch;

/// Boot envs
pub(crate) mod bectl;

/// Filesystem stuff (mostly flags related)
mod fs;
pub(crate) use fs::{lchflags, unschg_file};
pub(crate) use fs::{lstat, LstatErr};



// XXX Is caching worth it?  geteuid() may not even be an actual syscall
// now, so may be cheaper than eating the atomic...
use std::sync::atomic::AtomicU32;
static EUID: AtomicU32 = AtomicU32::new(0);

pub(crate) fn set_euid()
{
	use std::sync::atomic::Ordering::Relaxed;
	EUID.store(uzers::get_effective_uid(), Relaxed);
}

/// We'll care about euid for things like chown() calls.
pub(crate) fn euid() -> u32
{
	use std::sync::atomic::Ordering::Relaxed;
	EUID.load(Relaxed)
}



/// For writing out files, we may want some buffering.  In a little quick
/// sampling, over 99% of the files are sub-1 meg, and 4 megs gets us to
/// something like 99.8%.  So that's a good working number for a buffer
/// size to cut down on syscalls etc...
pub(crate) static FILE_BUFSZ: usize = 4 * 1024 * 1024;



use std::path::{Path, PathBuf};

/// Append paths.
///
/// It's not trivial to just use Path::join() because it treats join'ing
/// an "absolute" path as _replacing_ the base, not appending to.
/// Presumable there are usecases where that's the sensible behavior.
/// For us, though, it pretty much never is; we're always treating the
/// base path as a sort of "chroot".  So to avoid repeating ourselves too
/// often, just make a util func for it.
pub(crate) fn path_join(base: impl AsRef<Path>, sub: impl AsRef<Path>)
		-> PathBuf
{
	// So for our subpath, strip off the leading absoluteness if it has
	// it.
	let sub = match sub.as_ref().strip_prefix("/") {
		Ok(x) => x,
		Err(_) => sub.as_ref(),
	};
	// Then .join will do what we want.
	base.as_ref().join(sub)
}



/// Uniq-ify a set of Vec's.
///
/// This empties out the incoming Vec's because curent consumers don't
/// need them after, and this saves doing a bunch of Clone'ing.
pub(crate) fn uniq_vecs<T>(vs: &mut [Vec<T>]) -> Vec<T>
		where T: Eq + std::hash::Hash
{
	// Just go through a HashSet to uniqify.  It's probably not the most
	// efficient possible implementation, but hey...
	use std::collections::HashSet;

	let mut ceil = 0;
	for v in vs.iter() { ceil += v.len(); }

	let mut hs = HashSet::with_capacity(ceil);
	for v in vs { hs.extend(v.drain(..)); }

	let v: Vec<T> = hs.into_iter().collect();
	v
}


/// argv[0]
pub(crate) fn argv_0() -> Option<std::ffi::OsString>
{
	std::env::args_os().next()
}


/// What's our command's name?  With fallback...  this is mostly intended
/// for cosmetic use, like telling the user to "run this command".
pub(crate) fn cmdname() -> String
{
	argv_0()
		.and_then(|c| Some(c.to_string_lossy().into_owned()))
		.and_then(|s| Some(s.split('/').next_back()?.to_string()))
		.unwrap_or_else(|| "freebsd-rustdate".to_string())
}


/// Pluralize for a number
pub(crate) fn plural(n: usize) -> &'static str
{
	if n == 1 { "" } else { "s" }
}


/// Is a given path kernel-y?  Used in the install process for Upgrades,
/// as we split the install into multiple steps.
pub(crate) fn is_kernel_dir(p: &impl AsRef<Path>) -> bool
{
	p.as_ref().starts_with("/boot")
}
