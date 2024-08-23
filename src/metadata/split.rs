//! SplitTypes handling
use std::path::PathBuf;
use std::collections::HashMap;

use crate::metadata::Metadata;
use crate::metadata::MetadataLine;



/*
 * Split out types: this is mostly used in install-like processes.
 */
#[derive(Debug, Default)]
pub(crate) struct SplitTypes
{
	pub(crate) dirs:  HashMap<PathBuf, MetadataLine>,
	pub(crate) files: HashMap<PathBuf, MetadataLine>,
	pub(crate) syms:  HashMap<PathBuf, MetadataLine>,
	pub(crate) hards: HashMap<PathBuf, MetadataLine>,
	pub(crate) flags: HashMap<PathBuf, MetadataLine>,
}


impl SplitTypes
{
	/// Prematurely microoptimize these sizes by wild guesses
	pub(crate) fn sized(sz: usize) -> Self
	{
		// Most of what's installed is files, but there can also be a lot
		// of hardlinks.  Directories and symlinks, a lot less.  And
		// there's hardly ever more than small single-digit numbers of
		// flags to set.
		let sz = std::cmp::min(sz, 128);
		Self {
			files: HashMap::with_capacity(sz / 2),
			hards: HashMap::with_capacity(sz / 4),
			syms:  HashMap::with_capacity(sz / 8),
			dirs:  HashMap::with_capacity(sz / 8),

			flags: HashMap::with_capacity(8),
		}
	}


	/// Build from a existing list of (paths, lines)
	pub(crate) fn from_map_lines(mds: HashMap<PathBuf, MetadataLine>)
			-> Self
	{
		let mut ret = Self::sized(mds.len());

		for (path, mdl) in mds.into_iter()
		{
			// If it has flag to set, copy it into that list
			if mdl.has_flags() { ret.flags.insert(path.clone(), mdl.clone()); }

			// Whatever else it is, put it on that list.
			use MetadataLine as ML;
			match mdl
			{
				ML::Dir(_)      => { ret.dirs.insert(path, mdl); },
				ML::File(_)     => { ret.files.insert(path, mdl); },
				ML::SymLink(_)  => { ret.syms.insert(path, mdl); },
				ML::HardLink(_) => { ret.hards.insert(path, mdl); },
				_ => (),  // Dash line?  How could that happen...
			}
		}

		ret
	}
}


impl From<Metadata> for SplitTypes
{
	fn from(m: Metadata) -> Self
	{
		// We know the sizes for most of our bits.  We guess a smallish
		// starting point for flags
		let mut files = HashMap::with_capacity(m.files.len());
		let mut dirs  = HashMap::with_capacity(m.dirs.len());
		let mut syms  = HashMap::with_capacity(m.symlinks.len());
		let mut hards = HashMap::with_capacity(m.hardlinks.len());
		let mut flags = HashMap::with_capacity(8);

		// Now turn our bits over into it.  First check the flags, then
		// just consume everything in.
		for (p, m) in &m.files
		{ if m.flags != 0 { flags.insert(p.to_path_buf(), m.clone().into()); } }
		for (p, m) in &m.dirs
		{ if m.flags != 0 { flags.insert(p.to_path_buf(), m.clone().into()); } }
		for (p, m) in &m.symlinks
		{ if m.flags != 0 { flags.insert(p.to_path_buf(), m.clone().into()); } }

		for (p, m) in m.files     { files.insert(p, m.into()); }
		for (p, m) in m.dirs      { dirs.insert(p, m.into()); }
		for (p, m) in m.symlinks  { syms.insert(p, m.into()); }
		for (p, m) in m.hardlinks { hards.insert(p, m.into()); }

		Self { files, dirs, syms, hards, flags }
	}
}
