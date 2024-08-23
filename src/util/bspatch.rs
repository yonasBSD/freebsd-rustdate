//! Apply bspatches
//!
//! The currently available choices seem to mostly boil down to invoking
//! bspatch(1) externally, and the qbsdiff crate.
use std::path::Path;


/// Try to patch an input file into an output file.
pub(crate) fn patch(src: &Path, dst: &Path, patch: &Path)
		-> Result<(), std::io::Error>
{
	match true {
		true  => patch_qbsdiff(src, dst, patch),
		false => patch_ext(src, dst, patch),
	}
}


/// Try to patch an input file into an output file via bspatch(1)
pub(crate) fn patch_ext(src: &Path, dst: &Path, patch: &Path)
		-> Result<(), std::io::Error>
{
	use std::io::ErrorKind as EK;

	let ret = std::process::Command::new("/usr/bin/bspatch")
			.arg(src).arg(dst).arg(patch).status()?;
	match ret.success() {
		true => Ok(()),
		false => Err(EK::Other)?,
	}
}


/// Try to patch an input file into an output file via qbsdiff
pub(crate) fn patch_qbsdiff(src: &Path, dst: &Path, patch: &Path)
		-> Result<(), std::io::Error>
{
	use std::fs::{self, File};
	use qbsdiff::Bspatch;

	// let srcf = File::open(src)?;
	// https://github.com/hucsmn/qbsdiff/pull/8
	// Until then...
	let srcb = fs::read(src)?;
	let mut dstf = File::create(dst)?;
	let patchb = fs::read(patch)?;

	let patcher = Bspatch::new(&patchb)?;
	patcher.apply(&srcb, &mut dstf).and_then(|_| Ok(()))
}
