//! Higher-level rolled up install routines.
//!
//! These were originally just internals of src/install.rs, but are big
//! enough and otherwise useful enough to share.
use crate::core::RtDirs;
use crate::metadata::MetadataLine;
use crate::metadata::SplitTypes;
use crate::util::{plural, path_join};
use crate::core::install as install;

use std::io::{stdout, Write as _};
use std::collections::HashMap;
use std::path::{Path, PathBuf};



// Predef some RE's for reuse.  Too bad this isn't const...  I guess I
// could do some Once tricks.
use regex_lite::Regex;
pub(crate) fn re_linker_file() -> Regex
{
	Regex::new(r"^/libexec/ld-elf.*\.so\.[0-9]+$")
		.expect("I can rite regex")
}
pub(crate) fn re_so_file() -> Regex
{
	Regex::new(r".*/lib/.*\.so\.[0-9]+$")
		.expect("I can rite regex")
}



/// Once we have a SplitTypes, install it all.
pub(crate) fn split(smd: SplitTypes, rtdirs: &RtDirs, basedir: &Path,
		dry: bool)
		-> Result<(), anyhow::Error>
{
	// Now start installing the bits.  f-u.sh just goes through the
	// manifest lexically and splats things in place.  I'm going to do it
	// by type instead; handle all the dirs, then the files, then the
	// links.
	//
	// Maybe should look at setting up threadpools for this, but it's not
	// quite trivial; we have to worry about ordering issues.  At least
	// for dirs...   hm.  Revisit this.
	let dry_do_one = |hm: &HashMap<_, _>|
			-> Result<(), anyhow::Error> {
		match dry {
			true => {
				println!("  (dry run, not installing)");
				Ok(())
			},
			false => do_mdl_installs(hm, rtdirs, basedir)
		}
	};

	let dlen = smd.dirs.len();
	let flen = smd.files.len();
	let slen = smd.syms.len();
	let hlen = smd.hards.len();

	if dlen > 0
	{
		println!("{} director{}", dlen,
			if dlen > 1 { "ies" } else { "y" });
		dry_do_one(&smd.dirs)?;
	}

	if flen > 0
	{
		println!("{} file{}", flen, plural(flen));
		dry_do_one(&smd.files)?;
	}

	if slen > 0
	{
		println!("{} symlink{}", slen, plural(slen));
		dry_do_one(&smd.syms)?;
	}

	if hlen > 0
	{
		println!("{} hardlink{}", hlen, plural(hlen));
		dry_do_one(&smd.hards)?;
	}



	// Second pass: set schg flags.
	let flen = smd.flags.len();
	if flen > 0 && dry
	{
		println!("{flen} flag{}  (dry run)", plural(flen));
	}
	else if flen > 0 && crate::util::euid() != 0
	{
		println!("Not setting {flen} flag{} because you're not root.",
				plural(flen));
	}
	else if flen > 0
	{
		// Not bothering to progress this, there's rarely
		// non-single-digit.
		print!("Setting {flen} flag{}...  ", plural(flen));
		stdout().flush()?;
		for (p, mdl) in &smd.flags
		{
			let flags = mdl.flags().expect("Must exist if we get here");
			install::flags(p, flags)?;
		}
		println!("Done.");
	}

	Ok(())
}



use indicatif::ProgressBar;

/// Iterate over a set of MetadataLine's, doing the installs.
///
/// In practice, when we call this, all the MetadataLine's are of a
/// single type, since we do one type at a time.  The generic
/// MetadataLine layer is really just to let us wrap up all the stuff
/// that happens _around_ installing the lines.
///
/// This is slightly complicated by the desire to do things in a
/// particular order.  Since we get run here via install_split(), it
/// already handles the "do dirs first" etc bits of the ordering.  So
/// we'll impl f-u.sh's ordering that mostly applies to files.
///
/// Strictly, this gives us a different answer, because we're not
/// intermingling files, symlinks, and hardlinks like f-u.sh is.  It
/// feels like there are drawbacks both ways though, so I'm going to
/// forge ahead.
fn do_mdl_installs(hm: &HashMap<PathBuf, MetadataLine>, rtdirs: &RtDirs,
		basedir: &Path) -> Result<(), anyhow::Error>
{
	use crate::metadata::MetadataLine as ML;

	let pb = ProgressBar::new(hm.len().try_into().unwrap());


	// If one entry is a dir, they're all dirs, so just shortcut and make
	// them all.
	if let Some(m) = hm.values().next()
	{
		match m
		{
			ML::Dir(_) => {
				let paths: Vec<_> = hm.keys().sorted().collect();
				let ret = do_mdl_installs_inner(&paths, &pb, hm,
						rtdirs, basedir);
				pb.finish();
				return ret;
			},
			_ => (),
		}
	}


	// As f-u.sh does, split into 3 batches: runtime linker, shared libs,
	// other stuff.  Very few linkers, up to a couple hundred libs, and
	// however much there is everything else, as our size guesses.
	let mut lds    = Vec::with_capacity(2);
	let mut shlibs = Vec::with_capacity(128);
	let mut rest   = Vec::with_capacity(hm.len());

	// What're the rules...
	let linker = re_linker_file();
	let shlib = re_so_file();

	use itertools::Itertools as _;
	for p in hm.keys().sorted()
	{
		// regex_lite really wants a &str for matching, so str-ify our
		// path I guess...
		let pstr = String::from_utf8_lossy(p.as_os_str().as_encoded_bytes());

		if linker.is_match(&pstr)
		{ lds.push(p); }
		else if shlib.is_match(&pstr)
		{ shlibs.push(p); }
		else
		{ rest.push(p); }
	}


	// OK, now go through 'em in order
	let doit = |v| {
		do_mdl_installs_inner(v, &pb, hm, rtdirs, basedir)
	};
	doit(&lds)?;
	doit(&shlibs)?;
	doit(&rest)?;


	// And that's it
	pb.finish();
	Ok(())
}

fn do_mdl_installs_inner(paths: &[impl AsRef<Path>], pb: &ProgressBar,
		hm: &HashMap<PathBuf, MetadataLine>, rtdirs: &RtDirs,
		basedir: &Path) -> Result<(), anyhow::Error>
{
	use crate::metadata::MetadataLine as ML;

	for p in paths
	{
		let mdl = hm.get(p.as_ref()).unwrap();
		let dst = path_join(basedir, p);
		match mdl
		{
			ML::Dir(m)      => install::dir(&dst, m)?,
			ML::File(m)     => install::file(&dst, m, &rtdirs)?,
			ML::SymLink(m)  => install::symlink(&dst, m)?,
			ML::HardLink(m) => install::link(&dst, m, basedir)?,
			_ => unreachable!("Impossible!"),
		}
		pb.inc(1);
	}

	Ok(())
}
