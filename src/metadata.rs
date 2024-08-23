//! Metadata file contents
//!
//! The various INDEX-foo files are all in a pipe-delimited format.  The
//! details are...   well, "guessed" sounds weak.  I'm gonna say
//! "derived and inferred from intensive study", that makes me sound
//! smarter.


/// Metadata index stuff
mod idx;
pub(crate) use idx::MetadataIdx;

/// Structs for the info
mod structs;
pub(crate) use structs::{MetaFile, MetaHardLink, MetaDir, MetaSymLink};
use structs::MetaDash;
pub(crate) use structs::Metadata;

/// Full parsing
mod parse;
pub(crate) use parse::ParseFileErr;

/// MetadataGroup handling; this is most of the things related to a given
/// metadata file.
mod group;
pub(crate) use group::MetadataGroup;

/// MetadataLine handling; this is an abstract container for any sorta of
/// MetaX.  Used in parsing, and some places where we want a generic
/// container.
mod line;
pub(crate) use line::MetadataLine;
pub(crate) use line::MetaChange;
//pub(crate) use line::MetadataLineDiff;

/// Metadata handling; once we've dealt with components, we do a lot on
/// the collected Metadata ifself.
mod metadata;

/// Handling of files in a metadata; this covers things like stashing up
/// current files.
mod files;

/// SplitTypes handling; used in various install-like processes.
mod split;
pub(crate) use split::SplitTypes;




/*
 * Some misc utils that don't deserve their own place.
 */

// Some type renames we use around metadata stuff.  These are just for
// convenience.  In practice, some of them are actually u16's for us, but
// some things are easier if we just call 'em all u32's.
#[allow(non_camel_case_types)]
pub(crate) type uid_t   = u32;
#[allow(non_camel_case_types)]
pub(crate) type gid_t   = u32;
#[allow(non_camel_case_types)]
pub(crate) type mode_t  = u32;
#[allow(non_camel_case_types)]
pub(crate) type flags_t = u32;

use std::fmt;
fn oct_fmt_u32(o: &u32, f: &mut fmt::Formatter) -> fmt::Result
{
	write!(f, "O{o:o}")
}
fn hex_fmt_u32(o: &u32, f: &mut fmt::Formatter) -> fmt::Result
{
	write!(f, "Ox{o:x}")
}



// Util: When comparing Metadata types, to support running as non-root,
// if you're not root, we assume root doesn't own the destination either,
// which means everything will be owned by you and whatever group makes
// sense.  So if you're not root, we ignore uid/gid for stuff.
use std::sync::atomic::{Ordering, AtomicBool};
static UGID_COMPARE: AtomicBool = AtomicBool::new(true);

fn cmp_ugid(a: &u32, b: &u32) -> bool
{
	match UGID_COMPARE.load(Ordering::Relaxed) {
		true  => a == b,
		false => true,
	}
}

pub(crate) fn set_ugid_cmp(v: bool)
{
	UGID_COMPARE.store(v, Ordering::Relaxed);
}

// By default, we ignore u/gid perms in comparisons if you're not root.
pub(crate) fn init_ugid_cmp()
{
	set_ugid_cmp(crate::util::euid() == 0)
}
