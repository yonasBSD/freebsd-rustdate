//! Metadata file related structs.
//!
//! These have multiple lines of at least 3 known types; files,
//! directories, and symlinks.  And files may be hardlinks.  It's
//! probably simplest to just treat them separately.
use std::path::PathBuf;
use std::collections::{HashMap, HashSet};


/*
 * Complete metadata collections.
 */

/// A complete set of metadata.  This will generally be "all the stuff
/// from <some file>", already broken down by component, so a given
/// metadata file probably has a handful of these.  As part of building
/// this, we'll have already sorted the individual sections so that e.g.
/// iterating over the dirs will always hit the parent before the child.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Metadata
{
	/// All the directories in this set
	pub(crate) dirs: HashMap<PathBuf, MetaDir>,

	/// All the symlinks
	pub(crate) symlinks: HashMap<PathBuf, MetaSymLink>,

	/// The files
	pub(crate) files: HashMap<PathBuf, MetaFile>,

	/// And the hardlinks
	pub(crate) hardlinks: HashMap<PathBuf, MetaHardLink>,

	/// Also the dash lines
	pub(crate) dashes: HashSet<PathBuf>,
}





/*
 * Stuff for individual lines of metadata
 */
use crate::util::hash::Sha256Hash;
use super::{uid_t, gid_t, mode_t, flags_t};
use super::{oct_fmt_u32, hex_fmt_u32};
use super::cmp_ugid;


// n.b. for sorting purposes, the Ord/Eq bits are a bit excessive, since
// we often want to sort Vec's of these by the filename.  But since the
// derives are defined to go in order of the fields, the file is always
// first, and those sort of collections are already unique by the
// filename, they'll work out right for the sorting cases.


/// A metadata line about a [f]ile.  Notable this doesn't include the
/// lines that refer to hardlinks.
#[derive(Default, Clone, PartialOrd, Ord)]
#[derive(serde::Serialize, serde::Deserialize)]
#[derive(derivative::Derivative)]
#[derivative(PartialEq, Eq)]
#[derivative(Debug)]
pub(crate) struct MetaFile
{
	/// The file name this line is about
	pub(crate) path: PathBuf,

	/// SHA256 hash.
	pub(crate) sha256: Sha256Hash,

	// Misc FS metadata
	#[derivative(PartialEq(compare_with="cmp_ugid"))]
	pub(crate) uid:   uid_t,
	#[derivative(PartialEq(compare_with="cmp_ugid"))]
	pub(crate) gid:   gid_t,
	#[derivative(Debug(format_with="oct_fmt_u32"))]
	pub(crate) mode:  mode_t,
	#[derivative(Debug(format_with="hex_fmt_u32"))]
	pub(crate) flags: flags_t,
}


/// A metadata line about a hardlink.  These come from [f] lines, but
/// I'm separating them for easier processing and more compact
/// representation.  e.g., the hash doesn't matter on these lines, only
/// the [f] line that gives the original file, permissions on the other
/// names can't matter (unless they're different, which means they'd have
/// changed the original file in the first place, and that's just
/// screwy), etc.
#[derive(Debug, Default, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct MetaHardLink
{
	/// The destination filename
	pub(crate) path: PathBuf,

	/// The hardlink destination (hopefully, a file we already had a
	/// MetaFile about).
	pub(crate) target: PathBuf,
}


/// A metadata line about a [d]irectory
#[derive(Debug, Default, Clone, PartialOrd, Ord)]
#[derive(serde::Serialize, serde::Deserialize)]
#[derive(derivative::Derivative)]
#[derivative(PartialEq, Eq)]
pub(crate) struct MetaDir
{
	/// The dir name this line is about
	pub(crate) path: PathBuf,

	// Misc FS metadata
	#[derivative(PartialEq(compare_with="cmp_ugid"))]
	pub(crate) uid:   uid_t,
	#[derivative(PartialEq(compare_with="cmp_ugid"))]
	pub(crate) gid:   gid_t,
	#[derivative(Debug(format_with="oct_fmt_u32"))]
	pub(crate) mode:  mode_t,
	#[derivative(Debug(format_with="hex_fmt_u32"))]
	pub(crate) flags: flags_t,
}


/// A metadata line about a sym[L]ink
#[derive(Debug, Default, Clone, PartialOrd, Ord)]
#[derive(serde::Serialize, serde::Deserialize)]
#[derive(derivative::Derivative)]
#[derivative(PartialEq, Eq)]
pub(crate) struct MetaSymLink
{
	/// The file name this line is about (that is, the name that will
	/// _be_ a symlink).
	pub(crate) path: PathBuf,

	/// The target of the symlink
	pub(crate) target: PathBuf,

	// Misc FS metadata, though its meaning for symlinks is questionable.
	#[derivative(PartialEq = "ignore")]
	pub(crate) uid:   uid_t,
	#[derivative(PartialEq = "ignore")]
	pub(crate) gid:   gid_t,
	#[derivative(Debug(format_with="oct_fmt_u32"))]
	pub(crate) mode:  mode_t,
	#[derivative(Debug(format_with="hex_fmt_u32"))]
	pub(crate) flags: flags_t,
}


/// A metadata line about a...   file that's missing in a given state?
/// Comments seem to suggest this is used in old/new comparisons where
/// the file is new on the new side?  Perhaps as a somewhat hacky way of
/// dealing with how f-u kinda pretends it knows the state of the system,
/// even though it's just guessing?  We also use this in some scanning
/// code etc to indicate a missing file generally.
#[derive(Debug, Default, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct MetaDash
{
	/// About a file
	pub(crate) path: PathBuf,
}
