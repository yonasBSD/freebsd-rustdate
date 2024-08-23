//! Installing the individual bits


/*
 * Implementing installing individual items.  These are all immediate and
 * destructive operations; they put a thing where it's said to do,
 * possibly removing whatever was previously there.
 *
 * As with f-u.sh, these explicitly _don't_ set flags; we do that in a
 * separate pass.
 */
use crate::metadata::{MetaFile, MetaHardLink, MetaDir, MetaSymLink};
use crate::core::RtDirs;
use super::fsync;

use std::fs;
use std::path::Path;
use std::io::Error as IOErr;


/// Creating a directory.
pub(crate) fn dir(dst: &Path, d: &MetaDir) -> Result<(), IOErr>
{
	use std::os::unix;

	// It can happen that a thing is previously something else (usually a
	// file) and turns into a dir, so if it exists and isn't a dir,
	// pre-whack it.  e.g.,
	// https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=273661
	if exists(dst) && !is_dir(dst) { fs::remove_file(dst)?; }


	// Looking through DirBuilder and DirBuilderExt, it seems like I can
	// set modes, but not owners.  So I'll have to do that in a
	// separate step manually anyway.  So go ahead and make the dir if it
	// doesn't exist, then tweak perms as necessary.
	let mut made = false;
	if !exists(dst)
	{
		use unix::fs::DirBuilderExt as _;
		let mut db = fs::DirBuilder::new();
		db.recursive(true).mode(d.mode);
		db.create(dst)?;
		// Should all exist with the right mode now.
		made = true;
	}

	// So now it exists (either pre-existing, or just created), and it's
	// definitely a dir.  Now just check owner and maybe modes.
	let mode = match made {
		true  => Some(d.mode),
		false => None,
	};
	set_perms(dst, d.uid, d.gid, mode)?;

	// OK, done
	return Ok(());
}



/// Creating a file
pub(crate) fn file(dst: &Path, f: &MetaFile, rtdirs: &RtDirs)
		-> Result<(), anyhow::Error>
{
	// First off, we better have the input hashfile, so do a cheap
	// double-check.  It should be impossible to fail this (unless
	// something's racing us), since the 'install' command checked
	// hashfile existence right up front.
	let hbuf = f.sha256.to_buf();
	let hashfile = rtdirs.hashfile(&hbuf);
	let _ = hashfile.metadata()?;

	// If there's a dir there, kill it off (loudly)
	rm_dir(&dst)?;

	// Write things out into a tempfile
	let tmpfile = {
		use tempfile::Builder;

		// Make it in the dest dir, we can feel pretty safe about
		// rename(2).
		let dstdir = dst.parent().ok_or_else(|| {
				use std::io::ErrorKind;
				let ek = ErrorKind::NotFound;
				let dp = dst.display();
				IOErr::new(ek, format!("No parent dir for {dp}??"))
			})?;
		let (tfh, tpath) = Builder::new().tempfile_in(dstdir)?.keep()?;

		// Buffer it
		use std::io::BufWriter;
		use crate::util::FILE_BUFSZ;
		let mut tbw = BufWriter::with_capacity(FILE_BUFSZ, tfh);

		// Decompress the data into it
		rtdirs.decompress_hash_write(&hbuf, &mut tbw)?;

		// Tear down the buffer, and optionally fsync.
		let tfh = tbw.into_inner()?;
		if fsync() { tfh.sync_data()?; }

		// And let the fh fall out, keeping the name of the file
		tpath
	};

	// Set the perms as necessary
	set_perms(&tmpfile, f.uid, f.gid, Some(f.mode))?;

	// Now put the tmpfile in the final location, and we're done.
	std::fs::rename(&tmpfile, dst)?;
	Ok(())
}



/// Creating a hardlink
pub(crate) fn link(dst: &Path, l: &MetaHardLink, basedir: &Path)
		-> Result<(), IOErr>
{
	let tpath = crate::util::path_join(basedir, &l.target);

	// When making a hardlink, the target needs to exist; failure there
	// probably means we screwed something up badly...
	if !exists(&tpath)
	{
		use std::io::{Error, ErrorKind};
		let ek = ErrorKind::NotFound;
		let tp = tpath.display();
		let err = Error::new(ek, format!("Link target {tp} not found"));
		return Err(err);
	}

	// If there's a dir there, kill it off (loudly)
	rm_dir(&dst)?;

	// If anything else is there, check stuff
	if exists(dst)
	{
		// If it's already a hardlink to the target, there's nothing to
		// do.
		use std::os::unix::fs::MetadataExt as _;
		let lm = dst.symlink_metadata()?;
		let tm = tpath.symlink_metadata()?;
		if lm.dev() == tm.dev() && lm.ino() == tm.ino()
		{ return Ok(()); }

		// Otherwise, kill it off (quietly) and move on
		fs::remove_file(&dst)?;
	}

	// And make the link
	fs::hard_link(&tpath, &dst)?;

	Ok(())
}



/// Creating a symlink
pub(crate) fn symlink(dst: &Path, l: &MetaSymLink) -> Result<(), IOErr>
{
	// If there's a dir there, kill it off (loudly)
	rm_dir(&dst)?;

	// If anything else is there, check stuff
	if exists(dst)
	{
		// If it's already a symlink, and already pointing at the right
		// target, we're done.
		if dst.is_symlink() && dst.read_link()? == l.target
		{ return Ok(()); }

		// Otherwise, kill it off (quietly) and move on
		fs::remove_file(&dst)?;
	}

	// And make the link
	use std::os::unix::fs::symlink;
	symlink(&l.target, &dst)?;

	Ok(())
}



/// Setting flags on a file.
///
/// Calling this "install" is a little loose maybe, but hey...
///
/// It's maybe reasonable to interpret the flags we get told from the
/// server as "these flags should be set", not necessarily "the flags
/// should be set to specifically and only these", but f-u.sh uses the
/// latter, so what the heck...
pub(crate) fn flags(dst: &Path, flags: u32) -> Result<(), anyhow::Error>
{
	crate::util::lchflags(dst, flags as u64)
}



/// Deleting a thing.
///
/// Also a little loose on the meaning of "install", but hey...
///
/// We allow removing a dir to quietly fail, 'cuz that's a thing that
/// would happen, but use the return value to note that it happened so
/// maybe code can warn...  Might be nice to delve deeper and be sure
/// it's a "not empty" vs "no permissions"...
pub(crate) fn rm(f: &Path) -> Result<bool, IOErr>
{
	if !exists(f) { return Ok(false); }
	match is_dir(f)
	{
		true  => {
			match fs::remove_dir(f) {
				Ok(_) => (),
				Err(_) => return Ok(true),
			};
		},
		false => fs::remove_file(f)?,
	};
	Ok(false)
}




/// Loudly remove a conflicting directory.  This is used when
/// "installing" a file or symlink or such, and a pre-existing directory
/// with that name exists.  This is sorta the opposite of the handling in
/// dir() above for a file turning into a dir, except in this case we
/// complain about it.
///
/// XXX Maybe we should just move it aside.  Well, f-u.sh doen't...
///
/// f-u.sh dir_conflict()
fn rm_dir(d: &Path) -> Result<bool, IOErr>
{
	if !exists(d) || !is_dir(d) { return Ok(false); }
	println!("Removing conflicting directory {}", d.display());
	fs::remove_dir_all(d)?;
	Ok(true)
}


/// Set uid/gid/perms on a file (or dir, etc) as necessary.
fn set_perms(f: &Path, uid: u32, gid: u32, mode: Option<u32>)
		-> Result<(), IOErr>
{
	use crate::util::euid;
	use std::os::unix;
	use unix::fs::{MetadataExt as _, PermissionsExt as _};
	use unix::fs::chown;

	let md = f.metadata()?;

	// uid/gid get handled together
	let uid = match md.uid() == uid {
		true => None,
		false => Some(uid),
	};
	let gid = match md.gid() == gid {
		true => None,
		false => Some(gid),
	};
	if (uid.is_some() || gid.is_some()) && (euid() == 0)
	{ chown(f, uid, gid)?; }

	// Maybe we do mode
	if let Some(mode) = mode
	{
		if md.permissions().mode() != mode
		{
			let nperm = fs::Permissions::from_mode(mode);
			fs::set_permissions(f, nperm)?;
		}
	}

	Ok(())
}



/*
 * Wrappers: a lot of Path:: methods trasverse symlinks for
 * "convenience".  That's not very convenient for us...
 */

/// See if a path is a thing that seems to exist.
///
/// It seems like Path::exists() would do this.  However, if the path is
/// a symlink, it'll follow it, and if it points nowhere, it "doesn't
/// exist".  For our uses, we don't want that; if it's a symlink, it
/// "exists" whether it points somewhere or not.  So we have to be
/// smarter.
fn exists(p: &Path) -> bool
{
	// If it exists, it exists.  If it doesn't exist, but is a symlink,
	// it exists.  If it doesn't exist and isn't a symlink, try to get
	// the metadata.  If we get some, it exists, if not, we have a
	// descritve-ish IOErr to return...
	match p.exists() {
		true => true,
		false => {
			match p.is_symlink() {
				true => true,
				false => !fs::symlink_metadata(p).is_err(),
			}
		},
	}
}


/// Is it a dir?
///
/// Here we bypass the permission issues; we assume it exists, or we
/// don't care of it doesn't, we only care if it's an existing directory.
/// Or alternate, we've already used exists() or its equivalent if we
/// care.
fn is_dir(p: &Path) -> bool
{
	match fs::metadata(p) {
		Ok(d)  => d.is_dir(),
		Err(_) => false,
	}
}
