//! MetadataLine and related bits
//!
//! This is based around the MetadataLine structure, which abstracts over
//! a single MetaX of some type.  This gets used in the parsing process,
//! and in some places where we need to abstractly store some number of
//! MetaX's for comparison or installation or the like.
//!
//! It includes the MetaChange type, which gets used to display info to
//! the user about a path changing types.
//!
//! And there's the MetadataLineDiff, which is used to show the
//! differences between two MetadataLine's (presumptively for the same
//! path).  This gets used to in places like the `check-sys` command, to
//! show how one state (like the current system) differs from another
//! (like the upstream expectation).

use super::{MetaFile, MetaHardLink, MetaDir, MetaSymLink, MetaDash};


/// Wrapper around metadata line types.  This is originally just built
/// for parsing, but also gets used in places where we want to more
/// generically hold some bit of metadata about a path.
#[derive(Debug, Clone)]
pub(crate) enum MetadataLine
{
	File(MetaFile),
	HardLink(MetaHardLink),
	Dir(MetaDir),
	SymLink(MetaSymLink),
	Dash(MetaDash),
}


// Converting into a Line; should be trivial.
impl From<MetaFile> for MetadataLine
{ fn from(f: MetaFile) -> Self { Self::File(f) } }
impl From<MetaHardLink> for MetadataLine
{ fn from(f: MetaHardLink) -> Self { Self::HardLink(f) } }
impl From<MetaDir> for MetadataLine
{ fn from(f: MetaDir) -> Self { Self::Dir(f) } }
impl From<MetaSymLink> for MetadataLine
{ fn from(f: MetaSymLink) -> Self { Self::SymLink(f) } }
impl From<MetaDash> for MetadataLine
{ fn from(f: MetaDash) -> Self { Self::Dash(f) } }


// Some common methods
impl MetadataLine
{
	/// Util: what type is a MetadataLine
	pub(crate) fn ftype(&self) -> &'static str
	{
		match self {
			Self::File(_)     => "file",
			Self::HardLink(_) => "hardlink",
			Self::Dir(_)      => "directory",
			Self::SymLink(_)  => "symlink",
			Self::Dash(_)     => "dashline",  // Probably meaningless
		}
	}


	/// During install we want to keep a list of MetadataLine's that have
	/// flags that need setting, so this is a convenient place to do the
	/// lookup.
	pub(crate) fn has_flags(&self) -> bool
	{
		match self.flags() {
			Some(f) => f != 0,
			None    => return false,
		}
	}


	/// What flags are on this line (assuming it's a type that has
	/// flags).
	pub(crate) fn flags(&self) -> Option<u32>
	{
		match self {
			Self::File(m)     => Some(m.flags),
			Self::HardLink(_) => None,
			Self::Dir(m)      => Some(m.flags),
			Self::SymLink(m)  => Some(m.flags),
			Self::Dash(_)     => None,
		}
	}
}




/// Information about a changed Metadata type for a given path.
///
/// We get this out of a Metadata::type_changes() call.
#[derive(Debug, Clone)]
pub(crate) struct MetaChange
{
	/// The "old" info
	pub(crate) old: MetadataLine,

	/// The "new" info
	pub(crate) new: MetadataLine,
}




use super::{uid_t, gid_t, mode_t, flags_t};
use crate::util::hash::Sha256Hash;
use std::path::PathBuf;

/// Info about a difference between two MetadataLine's.  This will get
/// used in some comparisons when we want to know the details of how
/// things differ, e.g. for the `check-sys` command.
///
/// Each type should be assumed to contain the (self, other) values for
/// the diff-showing.
///
/// It's assumed that users of this already have a sense that these
/// _should_ be compared, so e.g. if they are for different `.path`'s, we
/// won't even check...
#[derive(Debug, Clone)]
pub(crate) enum MetadataLineDiff
{
	/// Hash differs  (files)
	Sha256(Sha256Hash, Sha256Hash),

	/// Owning uid  (files, dirs)
	Uid(uid_t, uid_t),

	/// Owning gid  (files, dirs)
	Gid(gid_t, gid_t),

	/// File mode  (files, dirs)
	Mode(mode_t, mode_t),

	/// Flags  (files, dirs)
	Flags(flags_t, flags_t),

	/// Target (sym/hard links)
	Target(PathBuf, PathBuf),
}

use std::fmt;
impl fmt::Display for MetadataLineDiff
{
	/// Describe a difference.
	///
	/// Assume that in our usage, "self" would be "current system's
	/// version", and "other" would be "expected state".  That makes this
	/// display mostly really targetted at `check-sys`.
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
	{
		use MetadataLineDiff as D;
		match self {
			D::Sha256(s, o) => {
				write!(f, "hash {s} expected {o}")
			},
			D::Uid(s, o) => {
				write!(f, "uid {s} expected {o}")
			},
			D::Gid(s, o) => {
				write!(f, "gid {s} expected {o}")
			},
			D::Mode(s, o) => {
				write!(f, "mode {s:o} expected {o:o}")
			},
			D::Flags(s, o) => {
				write!(f, "flags {s:o} expected {o:o}")
			},
			D::Target(s, o) => {
				let s = s.display();
				let o = o.display();
				write!(f, "target {s} expected {o}")
			},
		}
	}
}

impl MetadataLineDiff
{
	pub(crate) fn dtype(&self) -> &'static str
	{
		use MetadataLineDiff::*;
		match self {
			Sha256(_,_) => "hash",
			Uid(_,_)    => "uid",
			Gid(_,_)    => "gid",
			Mode(_,_)   => "mode",
			Flags(_,_)  => "flags",
			Target(_,_) => "target",
		}
	}
}


impl MetadataLine
{
	/// Compare two MetadataLine's and return details about how they
	/// differ.
	///
	/// Returns an error if the types differ; you should have already
	/// dealt with that...  will have the .ftype() values of self/other
	/// in that error.
	pub(crate) fn diff(&self, other: &Self)
			-> Result<Option<Vec<MetadataLineDiff>>,
				(&'static str, &'static str)>
	{
		use MetadataLine as L;
		let mut ret = Vec::new();

		// Abstract up extracting a given type
		macro_rules! badtype {
			() => { return Err((self.ftype(), other.ftype())) }
		}
		macro_rules! typed {
			($t: ident) => {
				match other {
					L::$t(x) => x,
					_ => badtype!(),
				}
			};
		}

		// And creating an error
		macro_rules! mkdiff {
			($s: ident, $o: ident) => {
				macro_rules! diff {
					($et: ident, $fld: ident) => {
						ret.push(MetadataLineDiff::$et($s.$fld.clone(),
								$o.$fld.clone()))
					};
				}
			};
		}


		// The comparison differs by type, so handle them individually.
		match self
		{
			// Dash lines are meaningless here...
			L::Dash(_s) => {
				// Do extract to check the type
				let _o = typed!(Dash);
				// Otherwise there's nothing to check.
				return Ok(None);
			},

			// Hard/symlinks only compare target
			L::HardLink(s) => {
				let o = typed!(HardLink);
				mkdiff!(s, o);
				if s.target != o.target { diff!(Target, target); }
			},
			L::SymLink(s) => {
				let o = typed!(SymLink);
				mkdiff!(s, o);
				if s.target != o.target { diff!(Target, target); }
			},

			// Dirs have a full set of ownership
			L::Dir(s) => {
				let o = typed!(Dir);
				mkdiff!(s, o);

				if s.uid != o.uid { diff!(Uid, uid); }
				if s.gid != o.gid { diff!(Gid, gid); }
				if s.mode != o.mode   { diff!(Mode, mode); }
				if s.flags != o.flags { diff!(Flags, flags); }
			},

			// Files have ownership and hashes
			L::File(s) => {
				let o = typed!(File);
				mkdiff!(s, o);

				if s.uid != o.uid { diff!(Uid, uid); }
				if s.gid != o.gid { diff!(Gid, gid); }
				if s.mode != o.mode   { diff!(Mode, mode); }
				if s.flags != o.flags { diff!(Flags, flags); }
				if s.sha256 != o.sha256 { diff!(Sha256, sha256); }
			},
		}


		match ret.len() {
			0 => Ok(None),
			_ => Ok(Some(ret)),
		}
	}
}



#[cfg(test)]
mod tests
{
	use std::path::PathBuf;

	use crate::metadata;


	#[test]
	fn metadataline_diff()
	{
		use metadata::MetadataLine as ML;
		use metadata::{MetaHardLink, MetaSymLink};

		// Differing types should error
		let path = PathBuf::from("/foo/bar");
		let target = PathBuf::from("/foo/baz");
		let hl = MetaHardLink { path: path.clone(), target: target.clone() };
		let sl = MetaSymLink  {
			path: path.clone(), target: target.clone(),
			..Default::default()
		};

		let hll = ML::HardLink(hl.clone());
		let sll = ML::SymLink(sl);
		let e = hll.diff(&sll).expect_err("Should error on type diff");
		assert_eq!(e.0, "hardlink");
		assert_eq!(e.1, "symlink");

		// Diff against the same should yield no change.
		let hll2 = hll.clone();
		let d = hll.diff(&hll2).expect("Shouldn't error");
		assert!(d.is_none(), "Expected no diffs");

		// Change the target, should be a diff
		let mut hl2 = hl.clone();
		let target2 = PathBuf::from("/foo/quux");
		hl2.target = target2.clone();
		let hll2 = ML::HardLink(hl2);
		let d = hll.diff(&hll2).expect("Should have gotten a diff");
		assert!(d.is_some(), "Expected diffs");
		let d = d.unwrap();
		assert_eq!(d.len(), 1, "Expected 1 diff");

		use super::MetadataLineDiff as MLD;
		assert!(matches!(d[0], MLD::Target(_, _)), "Should be a Target diff");
		match &d[0]
		{
			MLD::Target(s, o) => {
				assert_eq!(s, &target,  "self target is right");
				assert_eq!(o, &target2, "other target is right");
			},
			_ => unreachable!("We know it's a Target already"),
		}

		// Double check the display
		let expdis = format!("target {} expected {}", target.display(),
				target2.display());
		let gotdis = &d[0].to_string();
		assert_eq!(&expdis, gotdis, "Got right Display for differing target");
	}
}
