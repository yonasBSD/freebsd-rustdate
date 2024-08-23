//! Various filtering of metadata bits.
//!
//! This doesn't fit nearly into Metadata methods, since it's a lot of
//! collating them together.
use std::path::{Path, PathBuf};
use std::collections::HashSet;

use crate::metadata::Metadata;

use regex_lite::Regex;


/// Results of a UpdateIfUnmodified/presence check
#[derive(Debug)]
pub(crate) struct ModifiedPresentRet
{
	/// Files to remove from all 3 Metadata sets
	files: HashSet<PathBuf>,

	/// Hardlinks to remove from all 3 Metadata sets
	hlinks: HashSet<PathBuf>,

	/// Dash lines to remove from cur
	dashes: HashSet<PathBuf>,
}


/// Figure out what needs to be done to collate together several Metadata
/// sets to handle UpdateIfUnmodified settings and DTRT for not-present
/// files.
///
/// UpdateIfUnmodified is really an exclusion; it would be better called
/// DontUpdateIfModified, since updating (overwriting) is the default
/// action.  So the goal is, if the cur entry doesn't match either old or
/// new, it's modified somehow, and we should leave it alone.  So that
/// means we should just go ahead and remove it from old/new/cur, so
/// later steps don't bother looking at it.
///
/// This apples straightforwardly to files; we just compare the hashes.
/// For hardlinks, we compare the targets, and just hope that everybody
/// agrees on which is the "file" and which is the "hardlink".  Symlinks
/// and dirs we don't do anything with.
///
/// Then there's the "not-present" part of this check.  That looks at the
/// MetaDash lines in cur (files we looked for but didn't find).  If we
/// looked for it, that means it was either in old or new, so we checked
/// the current state.  So if it's in cur, but has no Dash entry in old,
/// that's a "Modification", and so we should DontUpdate it.  If it's
/// "new" in new, f-u will have put a dash entry in "old" to tell us it
/// was expected to not be present yet.
///
/// Returns the lists of paths to remove from files and hardlinks from
/// all 3, and dashes from cur.  Generally you'd just use this to pass
/// into apply_modified_present().
///
/// This corresponds to f-u.sh's fetch_filter_unmodified_notpresent().
pub(crate) fn modified_present(old: &Metadata, new: &Metadata,
		cur: &Metadata, uium: &[Regex], ignore: Option<&HashSet<&Path>>,
		cv_old: Option<&Metadata>)
		-> ModifiedPresentRet
{
	let mut files  = HashSet::new();
	let mut hlinks = HashSet::new();
	let mut dashes = HashSet::new();

	// Handler for ignoring
	let doignore = |p: &Path| -> bool {
		match ignore {
			None => false,
			Some(ignores) => ignores.contains(p),
		}
	};

	// So first, pull out the paths that match UpdateIfUnmodified to make
	// our working copies.
	let uim_cur = cur.with_filter_paths_regexps(&uium);

	// dir, symlink, it's just the presence of the name.  But, if
	// neither old nor new contained it, we wouldn't have scanned it
	// anyway, so WTF are we checking??

	// files we compare the hashes
	uim_cur.files.iter().for_each(|(p, f)| {
		// Ignored?
		if doignore(p) { return; }

		let ch = Some(f.sha256);
		let oh = old.files.get(p).and_then(|tf| Some(tf.sha256));
		let nh = new.files.get(p).and_then(|tf| Some(tf.sha256));

		// If everything matches, there's nothing to do.
		if ch == oh || ch == nh { return; }

		// On upgrade we may have a cv_old too
		if let Some(cvo) = cv_old
		{
			let oh = cvo.files.get(p).and_then(|tf| Some(tf.sha256));
			if ch == oh { return; }
		}

		// Otherwise we clear 'em all out
		files.insert(p.to_path_buf());
	});

	// hardlinks...  because of the way f-u.sh works, it tracks the
	// hashes, which means it'd find a diff based on that.  We don't,
	// 'cuz that would be stupid, so we should call it "not matching"
	// if the target is a file that we put in our rms I guess?
	uim_cur.hardlinks.iter().for_each(|(p, l)| {
		// Ignored?
		if doignore(p) { return; }

		// If it's present in either, then fine, move along
		if old.hardlinks.contains_key(p) { return; }
		if new.hardlinks.contains_key(p) { return; }

		// Is our target file in the files?  We're relying on
		// deterministically making he same choices as to which is the
		// file and which is the hardlink...  If it's not there, then
		// there's also nothing to do
		if !files.contains(&l.target)  { return; }

		// On upgrade we may have a cv_old too
		if let Some(cvo) = cv_old
		{
			if cvo.hardlinks.contains_key(p) { return; }
		}

		// Otherwise, we're killing it off.
		hlinks.insert(p.to_path_buf());
	});


	// Also any of the missing lines in _cur that aren't missing in
	// _old need to be cleared out.  This isn't actually related to
	// UpdateIfUnmodified, so it's kinda a quirk of f-u.sh that this
	// happens here...
	cur.dashes.iter().for_each(|p| {
		// If old or cv_old have a dash line for it, we know to do
		// nothing.
		if old.dashes.contains(p) { return; }
		if let Some(cvo) = cv_old
		{
			if cvo.dashes.contains(p) { return; }
		}

		// If it's not in old, there's nothing to do
		let found = false;
		let found = found || old.files.contains_key(p);
		let found = found || old.dirs.contains_key(p);
		let found = found || old.hardlinks.contains_key(p);
		let found = found || old.symlinks.contains_key(p);

		// XXX cv_old here too?  Unclear...

		// If we found it, it was a thing in old, which means we
		// "modified" things, so we want to keep our "modification" of
		// deleting it, so we keep it.  The logic in f-u.sh seems wrong
		// for this...
		if found { dashes.insert(p.to_path_buf()); }
	});


	ModifiedPresentRet {files, hlinks, dashes}
}


/// Apply the results of modified_notpresent().  Usually you'd want to
/// pair these up.
///
/// Returns a collapset HashSet of the affected paths; this is mostly
/// only used to display to the user later.
pub(crate) fn apply_modified_present(mpret: ModifiedPresentRet,
		old: &mut Metadata, new: &mut Metadata, cur: &mut Metadata)
		-> HashSet<PathBuf>
{
	let mut ret = HashSet::new();

	// files: remove from all
	mpret.files.into_iter().for_each(|p| {
		old.files.remove(&p);
		new.files.remove(&p);
		cur.files.remove(&p);
		ret.insert(p);
	});

	// hardlinks: remove from all
	mpret.hlinks.into_iter().for_each(|p| {
		old.hardlinks.remove(&p);
		new.hardlinks.remove(&p);
		cur.hardlinks.remove(&p);
		ret.insert(p);
	});

	// dashes: remove from new
	mpret.dashes.into_iter().for_each(|p| {
		new.files.remove(&p);
		new.dirs.remove(&p);
		new.hardlinks.remove(&p);
		new.symlinks.remove(&p);
		// f-u.sh doesn't include these in the 'modifiedfiles' output, so
		// we won't include it in our returns here either.  Which is a
		// little bad, since we're not updating a "modified" file, but
		// OTOH this is also how `f-u.sh fetch` fakes up its handling of
		// missing subcomponents....
		//ret.insert(p);
	});

	ret
}
