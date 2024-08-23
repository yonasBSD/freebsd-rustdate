//! Kernel backup handling
use std::path::{Path, PathBuf};


/// Top-level: do a backup
///
/// This is the "outside" bit of f-u.sh's backup_kernel().  Though note
/// that we don't conditionalize, and we don't allow configuring the
/// BackupKernelDir; I'm just hardcoding it all.
pub(crate) fn backup_kernel(basedir: &Path) -> Result<(), anyhow::Error>
{
	// XXX f-u.sh seems a little broken WRT ${BASEDIR} here; it always
	// uses the `kern.bootfile` result for the 'running kernel'.  But
	// that doesn't make a ton of sense when we're not installing in /.
	// So I'm gonna choose to break stride with it; if we're in /, we use
	// kernel::dir(), but otherwise I just hardcode /boot/kernel.  Maybe
	// we'd need to add config for this if it starts to matter...
	use std::ffi::OsStr;
	let slash: &OsStr = "/".as_ref();
	let srcdir: PathBuf = match basedir.as_os_str() == slash {
		true => crate::info::kernel::dir()?,
		false => "/boot/kernel".to_string(),
	}.into();

	// If there's no kernel in that place, there's nothing to backup.
	let skern = crate::util::path_join(basedir, &srcdir).join("kernel");
	if !skern.exists() { return Ok(()); }

	let bakdir = match backup_dir(basedir) {
		Some(p) => p,
		None => anyhow::bail!("Can't figure kernel backup dir"),
	};

	// JFDI
	do_backup(basedir, &srcdir, &bakdir)?;
	Ok(())
}



/// Backup one kernel dir to another.
///
/// Generally, this is in the form "backup running kernel to <bakdir>.
/// f-u.sh does this by doing some mtree dances, excluding certain files,
/// then doing some hardlink magic.  I'm skipping the .debug/.symbols
/// exclusion (no longer standard as of rev 05117b57a54ab, pre-11.0.  And
/// I'm just assuming it's a flat tree, as seems to be the common case,
/// so we don't need mtree shenanigans.  Just cross-link the files.
///
/// Roughly the active part of f-u.sh's backup_kernel().
fn do_backup(basedir: &Path, spath: &Path, dpath: &Path)
		-> std::io::Result<()>
{
	use crate::util::path_join;
	use std::fs;

	let src = path_join(basedir, spath);
	let dst = path_join(basedir, dpath);

	// Remove the destination path if it exists
	if dst.exists()
	{
		// JIC
		if !dst.is_dir()
		{
			// Fudge up an IO error.
			use std::io::{Error, ErrorKind};
			// EK::NotADirectory is probably a good choice, but so far
			// not stabilized.
			let ek = ErrorKind::AlreadyExists;
			let dp = dpath.display();
			let err = Error::new(ek, format!("{dp} is not a directory"));
			return Err(err);
		}

		// Whack it
		fs::remove_dir_all(&dst)?;
	}


	// Make the dest, with our little flag file.
	fs::create_dir(&dst)?;
	fs::File::create(dst.join(".freebsd-update"))?;


	// And hardlink over all the file/symlinks.  We currently silently
	// skip dirs, since they shouldn't be there, but if they are, we
	// choose not to blow up for now...
	//
	// Not attempting to handle <src> and <dst> dirs being on separate
	// filesystems until I have a reason to bother.
	for f in fs::read_dir(&src)?
	{
		// Dunno what sorta Error we'd get here, but it's probably pretty
		// fatal.
		let f = f?;

		// Dir?  Wacky....
		if f.file_type()?.is_dir() { continue };

		// Otherwise link it over
		let fsrc = f.path();
		let fname = match fsrc.file_name() {
			Some(n) => n,
			None    => continue,  // ???
		};
		let fdst = dst.join(fname);

		fs::hard_link(&fsrc, &fdst)?;
	}


	// Well that's it then.
	Ok(())
}



/// Find candidate backup kernel directory.
///
/// We replicate f-u.sh's logic here, though without the configurability
/// currently.  So it's `/boot/kernel.old`, and maybe ${THAT}[1-9] if
/// earlier names already exist and are flagged as "ours" by having a
/// file touched in it.
///
/// f-u.sh backup_kernel_finddir()
fn backup_dir(basedir: &Path) -> Option<PathBuf>
{
	use crate::util::path_join as pj;

	let kd = "/boot/kernel.old";
	let fufile = ".freebsd-update";

	// Common case
	let ddir = pj(basedir, kd);
	if !ddir.exists() || (ddir.is_dir() && ddir.join(fufile).is_file())
	{
		return Some(kd.into());
	}

	// Try bumping 'em.
	for i in 1..=9
	{
		let ndir = format!("{kd}{i}");
		let ddir = pj(basedir, &ndir);
		if !ddir.exists() || (ddir.is_dir() && ddir.join(fufile).is_file())
		{
			return Some(ndir.into());
		}
	}

	// I give up...
	None
}
