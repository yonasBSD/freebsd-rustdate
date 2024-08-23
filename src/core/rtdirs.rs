//! Runtime directory info.
use std::path::{PathBuf, Path};

use crate::state;
use crate::util::hash::Sha256HashBuf;


/// Runtime dirs.  This gives info about directories we may need to
/// access at runtime that are specific to a given invocation of $0.
#[derive(Debug)]
pub(crate) struct RtDirs
{
	/// State directory.
	///
	/// This lives in `workdir`.  One of these exists per `basedir` (on
	/// most systems, just one, for `/`), and is where various tracking
	/// info about what we've done previously for that system install
	/// lives.
	///
	/// `freebsd-update.sh` uses a lot of dirs named things like
	/// `$BDHASH-<something>` for this.  We stick everything for each
	/// basedir under one dir for simplicity.  And because what we do it
	/// in probably isn't very compatible with f-u.sh anyway.
	state: PathBuf,

	/// Downloaded files dir.
	///
	/// This is where we store the raw files we download from the
	/// freebsd-update servers; mostly, they're named something like
	/// `<hash>.gz`.  This should be shared with f-u.sh, since it's
	/// nothing either we or it _writes_ ourselves, just what we download
	/// from the server.  In principal, it's just a cache of downloaded
	/// stuff, so could be arbitrarily blown away between runs.
	files: PathBuf,

	/// Temporary dir.
	///
	/// This is created and (modulo serious problems) removed on each
	/// invocation.  We use this to _e.g._ cache decompressed files that
	/// we're going to pass through multiple times, etc.
	///
	/// Using the tempfile crate means the Drop impl for this will clear
	/// out the directory, so this should do a pretty good job of
	/// automatically cleaning up, unless we get kill'd or the like.
	tmp: tempfile::TempDir,
}



// Trivial getters
impl RtDirs
{
	// pub(crate) fn state(&self) -> &Path { &self.state }
	pub(crate) fn files(&self) -> &Path { &self.files }
	pub(crate) fn tmp(&self)   -> &Path { &self.tmp.as_ref() }

	/// Build the full path to a .gz file with a given hash in our files
	/// dir.
	///
	/// This doesn't guarantee anything about the existence of the file,
	/// just puts together the path from a hash.
	pub(crate) fn hashfile(&self, hb: &crate::util::hash::Sha256HashBuf)
			-> PathBuf
	{
		let hgz = format!("{hb}.gz");
		self.files().join(hgz)
	}
}


impl RtDirs
{
	/// Initialize all our runtime dir info.
	///
	/// In addition to creating the struct, this also ensures all the
	/// dirs exist with the appropriate permissions.
	pub(crate) fn init(basedir: &Path, workdir: &Path)
			-> Result<Self, std::io::Error>
	{
		// Try and guard against obvious programmer screwup of passing
		// dirs in the wrong order.  While it's often the case that
		// workdir is under basedir (e.g., system at / and workdir in
		// /var/db/freebsd-update), and probably often the case that
		// they're disjoint (e.g., system /othersys and workdir
		// /var/db/othersys-freebsd-update), it's probably never sensible
		// to have the basedir be _under_ the workdir.  While there's no
		// obvious reason it would be impossible, it sounds pretty
		// stupid, so I think it's a good tradeoff to not support that,
		// in favor of being extra careful against the programmer
		// screwing up.
		if basedir.starts_with(workdir)
		{
			panic!("Error: basedir {} seems to be inside workdir \
					{}; this probably means the programmer screwed \
					up my args.", basedir.display(), workdir.display());
		}

		// If basedir doesn't exist, WTF.  That shoulda been caught
		// before now anyway, but eat a stat...
		if !basedir.exists()
		{
			use std::io::{Error, ErrorKind as EK};
			let d_s = basedir.to_string_lossy();
			let ioe = Error::new(EK::NotFound, d_s);
			return Err(ioe);
		}

		// Workdir, well, we'll expect to create it if necessary
		dodir(workdir, Some(0o700))?;

		// files/ is under workdir
		let files = workdir.join("files");
		dodir(&files, None)?;

		// statedir is named after the basedir, and is under workdir
		let state = workdir.join(statesubdir(basedir));
		dodir(&state, Some(0o700))?;

		// tmpdir goes in a tmp/ dir
		let tmpdir = workdir.join("tmp");
		dodir(&tmpdir, Some(0o700))?;
		let tmp = tempfile::TempDir::new_in(&tmpdir)?;


		// OK, all setup.  Return ourselves
		let ret = RtDirs { state, files, tmp };
		Ok(ret)
	}



	/// Loading inter-run state.  This is stored in our state dir, so we
	/// access it through here.
	///
	/// This loader handles defaulting, so it useful in many commands
	/// that don't much care whether there's an existing state, but want
	/// to use it if there is.
	pub(crate) fn state_load(&self)
			-> Result<state::State, state::StateLoadErr>
	{
		match self.state_load_raw() {
			Ok(s) => Ok(s.unwrap_or_else(|| state::State::default())),
			Err(e) => match e {
				state::StateLoadErr::None => unreachable!("s_l_r already ate this"),
				e => Err(e),
			},
		}
	}


	/// Loading inter-run state (raw version).
	///
	/// This differs from `state_load()` in that it doesn't default, so
	/// is useful for cases where we care about knowing whether there was
	/// existing state or not.
	pub(crate) fn state_load_raw(&self)
			-> Result<Option<state::State>, state::StateLoadErr>
	{
		match state::load_from_dir(&self.state) {
			Ok(s) => Ok(Some(s)),
			Err(e) => match e {
				state::StateLoadErr::None => Ok(None),
				e => Err(e),
			},
		}
	}


	/// Write a state back into our statedir
	pub(crate) fn state_save(&self, state: &state::State)
			-> Result<(), state::StateLoadErr>
	{
		state::save_to_dir(&self.state, state)
	}



	/// Decompress a hash.gz file from our files dir into a `Writer`er.
	/// Probably usually a `BufWriter`, but hey, it's your (function)
	/// call...
	pub(crate) fn decompress_hash_write(&self, hash: &Sha256HashBuf,
			out: &mut impl std::io::Write) -> Result<(), anyhow::Error>
	{
		use crate::util::compress;

		// OK, we know the filenames to deal with.
		let src = self.hashfile(hash);
		compress::decompress_gz_write(&src, out)
	}



	/// Decompress a hash.gz file from our files dir into a named output.
	pub(crate) fn decompress_hash_file(&self, hash: &Sha256HashBuf,
			outfile: &Path) -> Result<PathBuf, anyhow::Error>
	{
		use crate::util::compress;

		// OK, we know the filenames to deal with.
		let src = self.hashfile(hash);
		compress::decompress_gz_file(&src, outfile)?;
		Ok(outfile.to_path_buf())
	}
}



// Helper for making all the dirs
fn dodir(dir: &Path, mode: Option<u32>) -> Result<(), std::io::Error>
{
	// Should be there.
	if !dir.exists()
	{
		use std::fs::DirBuilder;
		use std::os::unix::fs::DirBuilderExt;
		let mut db = DirBuilder::new();
		if let Some(m) = mode { db.mode(m); }
		db.create(dir)?;
	}

	// Should be a dir (in case it already existed as something else)
	if !dir.is_dir()
	{
		use std::io::{Error, ErrorKind as EK};
		let d_s = dir.to_string_lossy();
		// EK::NotADirectory is probably the best match, but that's
		// nightly-only...
		let ioe = Error::new(EK::AlreadyExists, d_s);
		Err(ioe)?;
	}

	// Should we force the mode?  Not going to at the moment; if it
	// pre-existed with another mode, I'm gonna assume that was the
	// user's intention.


	// OK then
	Ok(())
}


// Figuring statedir name.
fn statesubdir(basedir: &Path) -> PathBuf
{
	// freebsd-update.sh uses a `echo $BASEDIR | sha256` invocation to
	// generate a name for various dirs in $workdir.  Yes, including the
	// "\n" from echo.  There's no obvious reason we need to act exactly
	// the same, it just needs to be something bitstirred that's a safe
	// filename to work with.  So I'm just gonna go with the URL-safe
	// base64 encoding.
	//
	// Now, what if basedir is so long, this winds up being too long to
	// be a filename?  Well, poo...  I'll just worry about that when it
	// happens.  Probably by yelling at whoever triggered it.
	use base64::engine::general_purpose::URL_SAFE_NO_PAD as USNP;
	use base64::Engine as _; // Pull in traits

	let bdbytes = basedir.as_os_str().as_encoded_bytes();
	let bdh = USNP.encode(bdbytes);
	format!("state.{bdh}").into()
}
