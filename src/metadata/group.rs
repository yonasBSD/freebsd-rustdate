//! MetadataGroup bits
//!
//! This is a "higher level" version of the Metadata struct, with stuff
//! split out by component.  It's mostly used in earlier steps of the
//! process where we care about components.  Most code paths will do that
//! a little, but then have chosen which components they work with, and
//! converted down to a flat Metadata, which is simpler to work with.
use std::path::Path;
use std::collections::{HashMap, HashSet};

use crate::components::Component;
use super::Metadata;

use regex_lite::Regex;



/// A complete metadata group.  This is some number of components, each
/// with a set of Metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MetadataGroup
{
	/// Some set of components, each with the appropriate entries.
	///
	/// We expect to normally have a very small number of these, so
	/// there's a fair chance we could win on performance with just a
	/// Vec<> and linear searches, but...
	pub(super) md: HashMap<Component, Metadata>,
}



impl MetadataGroup
{
	/// Get a list of all the pathnames in this MetadataGroup.
	pub(crate) fn allpaths(&self) -> Vec<&Path>
	{
		// It's a bit inefficient just thunking this down, manually doing
		// the iteration and building ourself would save some
		// reallocations etc, but...
		let mut ret = Vec::new();
		ret.extend(self.md.iter()
			.map(|(_comp, md)| md.allpaths_hashset())
			.flatten());
		ret
	}


	/// How many paths are in us?
	pub(crate) fn len(&self) -> usize
	{
		self.md.values().map(|m| m.len()).sum()
	}


	/// Get a list of what components this Group has
	pub(crate) fn components(&self) -> HashSet<Component>
	{
		self.md.keys().map(|c| c.clone()).collect()
	}


	/// Given a list of files, gen a list of which components have >=
	/// half of their files existing in that list.
	pub(crate) fn components_check(&self, existing: &HashSet<&Path>)
			-> HashSet<Component>
	{
		let mut ret = HashSet::with_capacity(self.md.len());

		for (comp, md) in &self.md
		{
			// Somehwat coincidentally, _nodash() vs regular doesn't
			// matter here due to what self winds up containing, but it's
			// strictly more correct...
			let cpaths = md.allpaths_hashset_nodash();
			let same = cpaths.intersection(&existing);

			let ntot = cpaths.len();
			let nsame = same.count();
			if nsame * 2 >= ntot { ret.insert(comp.clone()); }
		}

		ret
	}


	/// Strip components we don't care about from a MetadataGroup.
	pub(crate) fn keep_components(&mut self, keep: &HashSet<Component>)
	{
		// It seems like we could just .intersection(), but since
		// component comparision isn't exactly implemented with == but
		// with Component.contains(), that doen't DTRT.  We need to
		// .contains() the entries individually.
		let should_keep = |comp: &Component| -> bool {
			keep.iter().any(|c| c.contains(comp))
		};

		self.md.retain(|comp, _| should_keep(comp))
	}


	/// Strip non-matching paths from a MetadataGroup.
	pub(crate) fn keep_paths_matching(&mut self, paths: &[Regex])
	{
		self.md.iter_mut()
				.for_each(|(_comp, md)| md.keep_paths_matching(paths))
	}


	/// Strip matching paths from a MetadataGroup.
	pub(crate) fn remove_paths_matching(&mut self, paths: &[Regex])
	{
		// Note for this and simliar funcs: the big regex crate would let
		// us search in &[u8]'s; regex_lite doesn't seem to.  I'll just
		// stick here and str-ify for now until it turns out to be a
		// problem or we get a lot of non-UTF8 paths.
		self.md.iter_mut()
				.for_each(|(_comp, md)| md.remove_paths_matching(paths))
	}


	/// Possibly rewrite kernel paths.  As best I can interpret f-u.sh's
	/// fetch_filter_kernel_names(), this is because KERNCONF="FOO" could
	/// be in /boot/kernel or in /boot/FOO or elsewhere.  So anything in
	/// the metadata file called /boot/${KERNCONF} we want to make a
	/// duplicated /boot/${RUNNINGKERNELDIR} line for, then remove the
	/// /boot/${KERNCONF} line if there's no such directory?
	///
	/// This seems to mostly only have meaning for upgrades, to handle
	/// when you're running a custom kernel, where it'll try to put stuff
	/// in /kernel/GENERIC or something?  I'm gonna bail on this until I
	/// know a good reason to try and suss it out...
	pub(crate) fn rewrite_kern_dirs(&mut self) -> Result<(), anyhow::Error>
	{
		use crate::info::kernel;

		let kerndir  = kernel::dir()?;
		let kernconf = kernel::conf()?;
		let has_kcdir = {
			// XXX We're not taking account of config.basedir.  But then,
			// neither does f-u.sh.
			let kcdir = format!("/boot/{}", kernconf);
			let kcpath: &std::path::Path = kcdir.as_ref();
			kcpath.is_file()
		};

		self.rewrite_kern_dirs_inner(&kerndir, &kernconf, has_kcdir);
		Ok(())
	}

	fn rewrite_kern_dirs_inner(&mut self, _kerndir: &str, _kernconf: &str,
			_has_kcdir: bool)
	{
		// We'll be changing some lines, removing others, and adding
		// additional copies for some, so this doesn't nearly match a
		// simple iter_mut() or retain_mut() or the like...
	}


	// /// Absorb in another MetadataGroup.  other wins when we have simliar
	// /// entries.
	// pub(crate) fn extend(&mut self, mut other: Self)
	// {
	// 	self.md.iter_mut().for_each(|(comp, md)| {
	// 		other.md.remove(comp).and_then(|o| Some(md.extend(o)));
	// 	})
	// }


	/// Convenience method to convert into a Metadata.  We already impl
	/// From for this, so this is more just for making it explicit
	/// inline.
	pub(crate) fn into_metadata(self) -> Metadata { self.into() }
}



// In many cases, once we've keep_components()'d a MetadataGroup, we no
// longer care about the component layer, so there's value in just
// flattening down a single list.
impl From<MetadataGroup> for Metadata
{
	fn from(g: MetadataGroup) -> Self
	{
		let mut md = Metadata::default();
		g.md.into_iter().for_each(|(_comp, v)| {
			md.dirs.extend(v.dirs);
			md.files.extend(v.files);
			md.symlinks.extend(v.symlinks);
			md.hardlinks.extend(v.hardlinks);
			md.dashes.extend(v.dashes);
		});
		md
	}
}




#[cfg(test)]
mod tests
{
	use crate::components::Component;

	use crate::components::tests::src_comp;
	use crate::components::tests::world_comp;
	use crate::components::tests::base_comp;
	use crate::components::tests::lib32_comp;

	#[test]
	fn keep_components()
	{
		// Build up a meaningful mdgroup
		let mdlines = r##"
src|src|/usr/src/COPYRIGHT|f|0|0|0644|0|871846b8e369beaa915910e3cdc8563997c4cfbfcbdbf8ab6012af15c8cc7dd0|
world|lib32|/etc/pam.d/xdm|f|0|0|0644|0|3c6c43e880a1215022980e169b1e044bfdb38c66d8c0b78622202b43506d51c4|
world|base|/etc/pam.d/xdm|f|0|0|0644|0|3c6c43e880a1215022980e169b1e044bfdb38c66d8c0b78622202b43506d51c4|
"##;

		let mut rdr = mdlines.as_bytes();
		let mdg = crate::metadata::parse::reader(&mut rdr).unwrap();

		// We got src/src, lib32, and base
		let src    = src_comp();
		let srcsrc = "src/src".parse::<Component>().unwrap();
		let lib32  = lib32_comp();
		let base   = base_comp();

		let mut exp_comps = vec![&srcsrc, &lib32, &base];
		exp_comps.sort();
		let mut got_comps: Vec<_> = mdg.md.keys().collect();
		got_comps.sort();

		assert_eq!(exp_comps, got_comps, "Got expected components");

		// Let's say there are the ones we're keeping; we should be left
		// with both of 'em.
		let keep_comps = [src, lib32].into_iter().collect();
		let mut tokeep = mdg.clone();
		tokeep.keep_components(&keep_comps);

		// We said to keep src, but the extant entry is srcsrc
		let exp_comps = vec![&srcsrc, &lib32];
		let mut got_comps: Vec<_> = tokeep.md.keys().collect();
		got_comps.sort();
		assert_eq!(exp_comps, got_comps, "Kept expected components");

		// Just asking for world should keep both base and lib32
		let keep_comps = [world_comp()].into_iter().collect();
		let mut tokeep = mdg.clone();
		tokeep.keep_components(&keep_comps);

		// We said to keep src, but the extant entry is srcsrc
		let exp_comps = vec![&base, &lib32];
		let mut got_comps: Vec<_> = tokeep.md.keys().collect();
		got_comps.sort();
		assert_eq!(exp_comps, got_comps, "Kept expected components");
	}


	#[test]
	fn remove_keep_paths_matching()
	{
		let mdlines = r##"
src|src|/a/b/c|f|0|0|0644|0|871846b8e369beaa915910e3cdc8563997c4cfbfcbdbf8ab6012af15c8cc7dd0|
src|src|/a/b|f|0|0|0644|0|871846b8e369beaa915910e3cdc8563997c4cfbfcbdbf8ab6012af15c8cc7dd0|
src|src|/a|f|0|0|0644|0|871846b8e369beaa915910e3cdc8563997c4cfbfcbdbf8ab6012af15c8cc7dd0|
src|src|/d/e/f|f|0|0|0644|0|871846b8e369beaa915910e3cdc8563997c4cfbfcbdbf8ab6012af15c8cc7dd0|
src|src|/d/e|f|0|0|0644|0|871846b8e369beaa915910e3cdc8563997c4cfbfcbdbf8ab6012af15c8cc7dd0|
src|src|/foo/bar|f|0|0|0644|0|871846b8e369beaa915910e3cdc8563997c4cfbfcbdbf8ab6012af15c8cc7dd0|
"##;
		let mut rdr = mdlines.as_bytes();
		let mdg = crate::metadata::parse::reader(&mut rdr).unwrap();

		// Should all be in src/src, so just pull it out.
		assert_eq!(mdg.md.len(), 1, "Only 1 component");

		// For simplicity
		let srcsrc = "src/src".parse().unwrap();
		macro_rules! md {
			() => { mdg.md[&srcsrc] };
			($st:ident) => { $st.md[&srcsrc] };
		}

		// Should be 6 files
		assert_eq!(md!().files.len(), 6, "6 files");

		// Few quick checks
		use std::collections::HashMap;
		let has = |files: &HashMap<_, _>, f: &str| {
			let f: &std::path::Path = f.as_ref();
			files.contains_key(f)
		};
		assert!(has(&md!().files,  "/a"),     "has /a");
		assert!(has(&md!().files,  "/a/b"),   "has /a/b");
		assert!(has(&md!().files,  "/a/b/c"), "has /a/b/c");
		assert!(!has(&md!().files, "/a/b/c/d"), "not has /a/b/c/d");

		// OK, now let's filter down.
		use regex_lite::Regex;

		// This should strip out /a/b and /a/b/c, leaving the other 4
		let re = Regex::new("^/a/b").unwrap();
		let mut noab = mdg.clone();
		noab.remove_paths_matching(&[re]);
		assert_eq!(md!(noab).files.len(), 4, "4 files left");
		assert!(!has(&md!(noab).files,  "/a/b/c"), "lost /a/b/c");
		assert!(!has(&md!(noab).files, "/a/b"),    "lost /a/b");
		assert!(has(&md!(noab).files, "/a"),       "kept /a");

		// Now what if we keep the /d/e's?
		let re = Regex::new("^/d/e").unwrap();
		let mut des = mdg.clone();
		des.keep_paths_matching(&[re]);
		assert!(has(&md!(des).files,  "/d/e/f"),   "kept /d/e/f");
		assert!(has(&md!(des).files,  "/d/e"),     "kept /d/e");
		assert!(!has(&md!(des).files, "/a"),       "lost /a");
		assert!(!has(&md!(des).files, "/foo/bar"), "lost /foo/bar");
	}
}
