//! Info about the state of an in-progress upgrade of some sort.
use std::path::PathBuf;
use std::collections::HashMap;

use crate::metadata::{self, MetadataIdx, Metadata};
use crate::info::version::AVersion;
use crate::core::merge;

use thiserror::Error;


/// The statefile where we store our state.  It'd be Rust-y to use TOML,
/// but I s'pose I'll just go with JSON to make it a little more
/// generally readable to people's outside tools.  I recommend you don't
/// _write_ into it...
const STATEFILE: &str = "freebsd_rustdate_state.json";


/// The current state of something.  Since doing an upgrade involves
/// multiple invocations, this is where we keep track of what we've done
/// and what later invocations will need to do.
#[derive(Debug, Default)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct State
{
	/// Saved up metadata index bits from earlier fetch runs.  This is
	/// used to...   ....   TBD
	pub(crate) meta_idx: Option<MetadataIdx>,

	/// A prep'd up manifest for an upgrade of some sort.
	pub(crate) manifest: Option<Manifest>,

	// XXX Will have stuff about cleaning up shared libs etc when we get
	// that far.
}


/// A staged up upgrade's manifest.  This is information about what
/// things need to be shuffled around do install the upgrade.
///
/// This is our equivalent to what f-u.sh stores in
/// fetch_create_manifest().
#[derive(Debug)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) enum Manifest
{
	/// For a Fetch
	Fetch(ManiFetch),

	/// Or an Upgrade
	Upgrade(ManiUpgrade),
}


/// The Manifest for a pending fetch (intra-version) upgrade.  e.g.,
/// 1.2-RELEASE-p1 to 1.2-RELEASE-p2.
#[derive(Debug)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ManiFetch
{
	// No state to track here.  In f-u terms, fetch's assume the
	// kernel is always compatible, and there aren't any removed
	// libraries or anything, so 'install' does everything it will ever
	// do in one go, and everything's totally fine...

	/// The current (well, as of the scan we did) state of the system.
	pub(crate) cur: Metadata,

	/// The new (as of the upgrade being installed) state.
	pub(crate) new: Metadata,

	/// What we think the new version will be.
	vers: AVersion,
}


/// The Manifest for a pending upgrade (inter-version) upgrade.  e.g.,
/// 1.2-RELEASE-p4 to 2.1-RELEASE.
#[derive(Debug)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ManiUpgrade
{
	// State of the upgrade as we go.  In f-u terms, unlike fetch,
	// upgrade will be (at least presumptively) a userland incompatible
	// with the current kernel, and may be removing some libs etc, so
	// 'install' is a multi-step process.

	/// Kernel has been installed (and presumably, the user has rebooted
	/// before re-running with this set)
	pub(crate) kernel: bool,

	/// World has been installed
	pub(crate) world: bool,
	

	// Now the more general "what we might do" bits.

	/// The current (well, as of the scan we did) state of the system.
	pub(crate) cur: Metadata,

	/// The new (as of the upgrade being installed) state.
	pub(crate) new: Metadata,

	/// What we think the new version will be.
	vers: AVersion,

	/// Info about files that were successfully merged; this means the
	/// 'new' entries above aren't the pristine upstream new, but a merge
	/// of our previous state.  This may be important for the user to
	/// see.
	pub(crate) merge_clean: HashMap<PathBuf, merge::Clean>,

	/// Files that were not successfully merged, but have conflicts that
	/// need to be resolved.  This needs to be emptied out before we can
	/// install this pending upgrade.
	pub(crate) merge_conflict: HashMap<PathBuf, merge::Conflict>,
}


/// A change summary to be displayed.  This isn't necessarily a lot of
/// _detail_, but it gives a reasonable overview.
#[derive(Debug)]
pub(crate) struct ManifestSummary
{
	/// Files to add
	pub(crate) added: Vec<PathBuf>,

	/// Files to delete
	pub(crate) removed: Vec<PathBuf>,

	/// Files to update
	pub(crate) updated: Vec<PathBuf>,
}


impl State
{
	/// Is an 'upgrade' (the specific command, not the general concept)
	/// currently in-progress?
	///
	/// This is mostly to provide a simple check in the 'fetch/upgrade'
	/// commands to bail if we might be in a weird state.
	pub(crate) fn upgrade_in_progress(&self) -> bool
	{
		use Manifest::*;
		match &self.manifest
		{
			None => false,
			Some(mup) => match mup {
				Fetch(_) => false,
				Upgrade(u) => {
					// If kernel is false, we haven't started installing,
					// but if it's true, we're in the middle of things.
					u.kernel
				},
			},
		}
	}
}


impl Manifest
{
	/// Create the manifest from current state and upgrade aspiration
	/// (from fetch command).
	pub(crate) fn new_fetch(cur: Metadata, new: Metadata, vers: AVersion)
			-> Self
	{
		let mf = ManiFetch { cur, new, vers };
		Self::Fetch(mf)
	}


	/// Create the manifest from current state and upgrade aspiration
	/// (from upgrade command).
	pub(crate) fn new_upgrade(cur: Metadata, new: Metadata, vers: AVersion,
			merge_clean: HashMap<PathBuf, merge::Clean>,
			merge_conflict: HashMap<PathBuf, merge::Conflict>
			)
			-> Self
	{
		let kernel = false;
		let world = false;
		let mu = ManiUpgrade { kernel, world,
				cur, new, vers, merge_clean, merge_conflict };
		Self::Upgrade(mu)
	}


	/// Generate a summary of the changes this manifest refers to, in
	/// terms of number of files removed, added, and updated.
	pub(crate) fn change_summary(&self) -> ManifestSummary
	{
		use std::collections::HashSet;

		// We gloss over the fetch/upgrade difference here.  These are
		// _nodash because we explicitly only care about paths that exist
		// on one side or the other.
		let curpaths: HashSet<_>;
		let newpaths: HashSet<_>;
		(curpaths, newpaths) = match self {
			Self::Fetch(f) => {
				(f.cur.allpaths_hashset_nodash(),
				f.new.allpaths_hashset_nodash())
			},
			Self::Upgrade(u) => {
				(u.cur.allpaths_hashset_nodash(),
				u.new.allpaths_hashset_nodash())
			},
		};

		// added = in new, not in cur.
		let mut added: Vec<_> = newpaths.difference(&curpaths)
				.map(|p| p.to_path_buf()).collect();

		// removed = in cur, not in new
		let mut removed: Vec<_> = curpaths.difference(&newpaths)
				.map(|p| p.to_path_buf()).collect();

		// updated = in both
		let mut updated: Vec<_> = curpaths.intersection(&newpaths)
				.map(|p| p.to_path_buf()).collect();

		// Sort and return.
		added.sort_unstable();
		removed.sort_unstable();
		updated.sort_unstable();
		ManifestSummary { added, removed, updated }
	}


	/// Show the type changes of a pending <whatever>
	pub(crate) fn type_changes(&self) -> HashMap<PathBuf, metadata::MetaChange>
	{
		match self {
			Self::Fetch(m)   => m.cur.type_changes(&m.new),
			Self::Upgrade(m) => m.cur.type_changes(&m.new),
		}
	}


	/// The version this thinks it will be
	pub(crate) fn version(&self) -> &AVersion
	{
		match self {
			Self::Fetch(f)   => &f.vers,
			Self::Upgrade(u) => &u.vers,
		}
	}

	/// Stringy type
	pub(crate) fn mtype(&self) -> &'static str
	{
		match self {
			Self::Fetch(_)   => "fetch",
			Self::Upgrade(_) => "upgrade",
		}
	}

	/// Say something about the state of the install process.  Right now
	/// this is just for output in show-install so it's all stringly
	/// typed.  Worry about being better when we need better.
	pub(crate) fn state(&self) -> &'static str
	{
		match self {
			Self::Fetch(_)  => {
				// No steps, it's just what it is
				"Ready to install"
			},
			Self::Upgrade(u) => {
				if !u.kernel
				{ "Ready to begin install" }
				else if !u.world
				{ "Kernel installed, ready to install world" }
				else
				{ "World installed, ready to clean up old shared libs" }
			},
		}
	}
}



impl ManiUpgrade
{
	pub(crate) fn num_clean(&self) -> usize    { self.merge_clean.len()    }
	pub(crate) fn num_conflict(&self) -> usize { self.merge_conflict.len() }


	/// For upgrades, since there may be merges, getting the info about
	/// what to install for a set of paths requires checking the merges
	/// as well as the new's.
	pub(crate) fn get_from_paths(&self, paths: Vec<PathBuf>)
			-> HashMap<PathBuf, crate::metadata::MetadataLine>
	{
		// Since we delegate the details down into
		// Metadata::get_from_paths(), we have to be a little indirect
		// about this.  So get the stuff from new.
		let mut ret = self.new.get_from_paths(paths);

		// Replace out of merge_clean.  Which should normally be tiny, so
		// loop over it on the outside...
		for (p, mrg) in self.merge_clean.iter()
		{
			if let Some(md) = ret.get_mut(p)
			{
				// Just replace the hash.  Which only makes sense if it's
				// a file, so don't bother otherwise.  Of course, we
				// wouldn't have a merge if it weren't a file, so maybe
				// this should be an error...
				use crate::metadata::MetadataLine as ML;
				match md
				{
					ML::File(f) => f.sha256 = (&mrg.res).into(),
					_ => (),
				}
			}
		}

		ret
	}
}



/// Error loading a state from a statefile
#[derive(Debug)]
#[derive(Error)]
pub(crate) enum StateLoadErr
{
	/// No state to load.  Not exactly an "error" usually, just means
	/// there's nothing to know.
	#[error("No state to load")]
	None,

	/// No extant statedir to write into.  This _is_ an error, but mostly
	/// a programming error; you should have a dir before you try writing
	/// a state to it...
	#[error("Can't write state: no statedir {0}")]
	NoDir(std::path::PathBuf),

	/// Some IO error (open, read, write, etc)
	#[error("Statefile I/O error: {0}")]
	IO(#[from] std::io::Error),

	/// Some sort of parsing error of the JSON
	#[error("Statefile parsing: {0}")]
	Parse(#[from] serde_json::Error),
}


/// Load current state from a statedir.  Mostly you'll be using this via
/// Config::state_load() instead.
pub(crate) fn load_from_dir(dir: &std::path::Path) -> Result<State, StateLoadErr>
{
	use StateLoadErr as SLE;

	// Find it, if it exists
	let statefile = dir.join(STATEFILE);
	if !statefile.is_file() { Err(SLE::None)? }

	// Open up and read
	// serde_json::from_str() is *crazy* faster than from_reader()...
	let sfstr = std::fs::read_to_string(&statefile)?;
	let state: State = serde_json::from_str(&sfstr)?;

	// Alright then
	Ok(state)
}


/// Write state out into a statedir.  Mostly you'll be using this via
/// Config::state_save() instead.
pub(crate) fn save_to_dir(dir: &std::path::Path, state: &State)
		-> Result<(), StateLoadErr>
{
	use StateLoadErr as SLE;

	// Statedir better exist
	if !dir.is_dir() { Err(SLE::NoDir(dir.to_path_buf()))? }

	// Open up the statefile
	// XXX Maybe someday we should worry about atomic updates.  Of
	// course, then we should probably be smarter than "blat a bit of
	// JSON" too, so...
	let statefile = dir.join(STATEFILE);

	// x-ref string vs reader stuff in load_from_dir above; it applies
	// here too.
	use std::io::Write as _;
	let mut sfwrite = std::fs::File::create(&statefile)?;
	let stjson = serde_json::to_string(state)?;
	sfwrite.write_all(stjson.as_ref())?;
	sfwrite.sync_all()?;

	// Alright then
	Ok(())
}
