//! Full parse of a metadata file
//!
//! These have multiple lines of at least 3 known types; files,
//! directories, and symlinks.  And files may be hardlinks.  It's
//! probably simplest to just treat them separately and have separate
//! lists for each metafile.
use std::path::Path;
use std::io::Read;

use super::{MetadataGroup, MetadataLine};
use crate::components::Component;

use anyhow::anyhow;
use anyhow::Error as AError;


/*
 * High level parsing whole blobs of metadata
 */
/// Error from parsing a metadata file
#[derive(Debug)]
#[derive(thiserror::Error)]
pub(crate) enum ParseFileErr
{
	#[error("I/O error: {0}")]
	IO(#[from] std::io::Error),

	#[error("Parse error: {0}: {1}")]
	Parse(u32, anyhow::Error)
}


/// A single parsed line from a metadata file.  For our purposes, the
/// component info is a thing we need in some places, and the details
/// about the individual record are another.  They're gonna be classified
/// at different levels, so we hold them separately in the parse.
#[derive(Debug)]
pub(crate) struct ParseLine
{
	/// What component (/sub) it's for
	pub(super) component: Component,

	/// What this entry is
	pub(super) mdline: MetadataLine,
}


/// Parse out a metadata file into a MetadataGroup of the info in it.
pub(crate) fn file(file: &Path)
		-> Result<MetadataGroup, Vec<ParseFileErr>>
{
	let mut fh = std::fs::File::open(file)
			.map_err(|e| vec![e.into()])?;
	reader(&mut fh)
}


/// Parse out metadata from a Read'er
pub(crate) fn reader(rdr: &mut impl Read)
		-> Result<MetadataGroup, Vec<ParseFileErr>>
{
	let lines = parse_reader_lines(rdr)?;

	Ok(lines.into())
}


// /// Parse out a metadata file into a stack of records
// fn parse_file_lines(file: &Path)
// 		-> Result<Vec<ParseLine>, Vec<ParseFileErr>>
// {
// 	let mut fh = std::fs::File::open(file)
// 			.map_err(|e| vec![e.into()])?;
// 	parse_reader_lines(&mut fh)
// }


/// Parse out a metadata file (as a Read'er) into a stack of records
fn parse_reader_lines(rdr: &mut impl Read)
		-> Result<Vec<ParseLine>, Vec<ParseFileErr>>
{
	use std::io::{BufRead, BufReader};

	let mut mds: Vec<ParseLine> = Vec::new();
	let mut errs: Vec<ParseFileErr> = Vec::new();

	let brdr = BufReader::new(rdr);
	let mut lnum = 0;
	for l in brdr.lines()
	{
		lnum += 1;
		let l = match l {
			Ok(l)  => l,
			Err(e) => { errs.push(e.into()); continue; },
		};

		if l.len() < 1 { continue; }
		let l = l.trim();

		use ParseFileErr::Parse as PEP;
		match l.parse()
		{
			Ok(md) => mds.push(md),
			Err(e) => errs.push(PEP(lnum, e)),
		}
	}

	match errs.len() {
		0 => Ok(mds),
		_ => Err(errs),
	}
}



/*
 * Getting a whole group from individual parsed lines
 */
impl From<Vec<ParseLine>> for MetadataGroup
{
	fn from(lines: Vec<ParseLine>) -> Self
	{
		let mut mdg = MetadataGroup::default();

		// into_iter()'ing should mean that we're not pop_front'ing and
		// moving everything every time, right?
		use super::MetadataLine as ML;
		for l in lines.into_iter()
		{
			let md = mdg.md.entry(l.component)
					.or_insert(Default::default());
			match l.mdline
			{
				ML::File(f)     => { md.files.insert(f.path.clone(), f); },
				ML::Dir(f)      => { md.dirs.insert(f.path.clone(), f); },
				ML::HardLink(f) => { md.hardlinks.insert(f.path.clone(), f); },
				ML::SymLink(f)  => { md.symlinks.insert(f.path.clone(), f); },
				ML::Dash(f)     => { md.dashes.insert(f.path.clone()); },
			}
		}

		mdg
	}
}



/*
 * Lower level handling of individual lines
 */
impl std::str::FromStr for ParseLine
{
	type Err = AError;

	fn from_str(s: &str) -> Result<Self, Self::Err>
	{
		use crate::components::{BaseComponent, BaseSubComponent};

		// Big pipe-separated line.
		let mut flds = s.split('|');

		// Different lines are different formats, but the first 4 fields
		// are always component, subcomponent, pathname, and type, so we
		// can pre-grab them.
		//
		// It seems like there's _always_ a subcomponent, so we just
		// presume we can grab it here...
		let comp: BaseComponent = flds.next()
				.ok_or_else(|| anyhow!("no component field"))?
				.parse().map_err(|e| anyhow!("Bad component: {e}"))?;
		let subcomp: BaseSubComponent = flds.next()
				.ok_or_else(|| anyhow!("no subcomponent field"))?
				.parse().map_err(|e| anyhow!("Bad subcomponent: {e}"))?;
		let subcomp = Some(subcomp);
		let component = Component { comp, subcomp };


		// Next the file/dir/whatever name
		let path = get_path(flds.next())?;


		// And now we have the type
		let rtype = flds.next().ok_or_else(|| anyhow!("no type field"))?;

		// So handle each type and get the MetadataLine
		let mdline = match rtype {
			"f" => {
				// A "file".  Or maybe a hardlink.

				// First, simple perms
				let uid   = get_uid(flds.next())?;
				let gid   = get_gid(flds.next())?;
				let mode  = get_mode(flds.next())?;
				let flags = get_flags(flds.next())?;

				// The hash
				let sha256 = get_sha256(flds.next())?;

				// And maybe a hardlink dest.  This _should_ always
				// succeed, but it _may_ be an empty string, which turns
				// into an empty path, so we'll have to use
				// .is_absolute() to differentiate.
				let hardlink = get_path(flds.next())?;

				// OK, we got everything; assemble whatever sorta return
				// we expect.
				use super::{MetaFile, MetaHardLink};
				match hardlink.is_absolute() {
					true  => {
						// Yep, it's a hardlink
						let mhl = MetaHardLink { path, target: hardlink };
						MetadataLine::HardLink(mhl)
					},
					false => {
						// It's a file
						let mf = MetaFile { path, sha256, uid, gid,
								mode, flags };
						MetadataLine::File(mf)
					},
				}
			},
			"d" => {
				// A "directory".

				// Has the same perms as a file
				let uid   = get_uid(flds.next())?;
				let gid   = get_gid(flds.next())?;
				let mode  = get_mode(flds.next())?;
				let flags = get_flags(flds.next())?;

				let md = super::MetaDir { path, uid, gid, mode, flags };
				MetadataLine::Dir(md)
			},
			"L" => {
				// Symlink time

				// Usual perms
				let uid   = get_uid(flds.next())?;
				let gid   = get_gid(flds.next())?;
				let mode  = get_mode(flds.next())?;
				let flags = get_flags(flds.next())?;

				// And a destination path
				let target = get_path(flds.next())?;

				// And that's a symlink
				let msl = super::MetaSymLink { path, target, uid, gid,
						mode, flags };
				MetadataLine::SymLink(msl)
			},
			"-" => {
				// This is...  a "known not present" or something?
				let md = super::MetaDash { path };
				MetadataLine::Dash(md)
			},
			_ => {
				// Dunno what this could be
				Err(anyhow!("Unexpected record type: {rtype}"))?
			},
		};

		Ok(Self { component, mdline })
	}
}


// Helpers for the parsing
fn get_path(s: Option<&str>) -> Result<std::path::PathBuf, AError>
{
	let s = s.ok_or_else(|| anyhow!("no path"))?;
	Ok(s.into())
}

use super::{uid_t, gid_t, mode_t, flags_t};
use crate::util::hash::Sha256Hash;
fn get_uid(s: Option<&str>) -> Result<uid_t, AError>
{
	let s = s.ok_or_else(|| anyhow!("no uid"))?;
	s.parse().map_err(|e| anyhow!("invalid uid: {e}"))
}

fn get_gid(s: Option<&str>) -> Result<gid_t, AError>
{
	let s = s.ok_or_else(|| anyhow!("no gid"))?;
	s.parse().map_err(|e| anyhow!("invalid gid: {e}"))
}

fn get_mode(s: Option<&str>) -> Result<mode_t, AError>
{
	let s = s.ok_or_else(|| anyhow!("no mode"))?;
	mode_t::from_str_radix(s, 8).map_err(|e| anyhow!("invalid mode: {e}"))
}

fn get_flags(s: Option<&str>) -> Result<flags_t, AError>
{
	let s = s.ok_or_else(|| anyhow!("no flags"))?;
	flags_t::from_str_radix(s, 8).map_err(|e| anyhow!("invalid flags: {e}"))
}

fn get_sha256(s: Option<&str>) -> Result<Sha256Hash, AError>
{
	let s = s.ok_or_else(|| anyhow!("no SHA256"))?;
	s.parse()
}



#[cfg(test)]
mod tests
{
	use super::ParseLine;

	#[test]
	fn file()
	{
		let inline = "world|base|/bin/[|f|0|0|0555|0|3ad985a50b79037b9672cf197fbc67bd54766199e190055101ea7d8c64ca843b|";
		let pl: ParseLine = inline.parse().expect("should parse ok");
		let ParseLine { component, mdline } = pl;

		// It's in world/base
		assert_eq!(component.comp.as_ref(), "world");
		assert_eq!(component.subcomp.unwrap().as_ref(), "base");

		// It's a File
		use super::MetadataLine as ML;
		let f = match mdline {
			ML::File(f) => f,
			_ => panic!("Shoulda been a File: {:?}", mdline),
		};

		// It's test, but under the wacky name
		assert_eq!(f.path.to_str().unwrap(), "/bin/[");

		// Reasonable expected owner and flags
		assert_eq!(f.uid, 0);
		assert_eq!(f.gid, 0);
		assert_eq!(f.mode, 0o555);
		assert_eq!(f.flags, 0);

		// And the right hash
		let hbuf = f.sha256.to_buf();
		assert_eq!(hbuf.as_ref(),
			"3ad985a50b79037b9672cf197fbc67bd54766199e190055101ea7d8c64ca843b");
	}


	#[test]
	fn hardlink()
	{
		let inline = "world|base|/bin/test|f|0|0|0555|0|3ad985a50b79037b9672cf197fbc67bd54766199e190055101ea7d8c64ca843b|/bin/[";
		let pl: ParseLine = inline.parse().expect("should parse ok");
		let ParseLine { component, mdline } = pl;

		// It's in world/base
		assert_eq!(component.comp.as_ref(), "world");
		assert_eq!(component.subcomp.unwrap().as_ref(), "base");

		// It's a Hardlink
		use super::MetadataLine as ML;
		let f = match mdline {
			ML::HardLink(f) => f,
			_ => panic!("Shoulda been a HardLink: {:?}", mdline),
		};

		// It's test, but under the obvious name
		assert_eq!(f.path.to_str().unwrap(), "/bin/test");

		// And hardlinks over to the wacky name
		assert_eq!(f.target.to_str().unwrap(), "/bin/[");
	}


	#[test]
	fn dir()
	{
		let inline = "world|base|/var/empty|d|0|0|0555|400000||";
		let pl: ParseLine = inline.parse().expect("should parse ok");
		let ParseLine { component, mdline } = pl;

		// It's in world/base
		assert_eq!(component.comp.as_ref(), "world");
		assert_eq!(component.subcomp.unwrap().as_ref(), "base");

		// It's a Directory
		use super::MetadataLine as ML;
		let f = match mdline {
			ML::Dir(f) => f,
			_ => panic!("Shoulda been a Dir: {:?}", mdline),
		};

		// It's empty!
		assert_eq!(f.path.to_str().unwrap(), "/var/empty");

		// Normal looking perms
		assert_eq!(f.uid, 0);
		assert_eq!(f.gid, 0);
		assert_eq!(f.mode, 0o555);

		// Oh, hey, this one has flags!  Let's play with bases just for
		// kicks...
		assert_eq!(f.flags, 0x20000);

	}


	#[test]
	fn dash()
	{
		let inline = "world|base|/nonexistent|-|||||";
		let pl: ParseLine = inline.parse().expect("should parse ok");
		let ParseLine { component, mdline } = pl;

		// It's in world/base
		assert_eq!(component.comp.as_ref(), "world");
		assert_eq!(component.subcomp.unwrap().as_ref(), "base");

		// It's a Dash
		use super::MetadataLine as ML;
		let f = match mdline {
			ML::Dash(f) => f,
			_ => panic!("Shoulda been a Dash: {:?}", mdline),
		};

		// It doesn't exist, which meant it exists
		assert_eq!(f.path.to_str().unwrap(), "/nonexistent");
	}


	#[test]
	fn bad_type()
	{
		let inline = "src|src|/baz|Q|";
		let err = inline.parse::<ParseLine>()
				.expect_err("shoulda failed");
		assert!(err.to_string().contains("Unexpected record type"),
				"should be row type error: {err}");
	}


	#[test]
	fn bad_component()
	{
		let inline = "foo|alreadyfailed";
		let err = inline.parse::<ParseLine>()
				.expect_err("shoulda failed");
		assert!(err.to_string().contains("Matching variant not found"),
				"should be component error: {err}");
	}


	#[test]
	fn bad_uid()
	{
		let inline = "world|base|/foo/var|f|notauid|0|...";
		let err = inline.parse::<ParseLine>()
				.expect_err("shoulda failed");
		assert!(err.to_string().contains("invalid uid"),
				"should be invalid uid: {err}");
	}


	#[test]
	fn symlink()
	{
		let inline = "kernel|generic|/boot/kernel/if_igb.ko|L|0|0|0755|0|if_em.ko|";
		let pl: ParseLine = inline.parse().expect("should parse ok");
		let ParseLine { component, mdline } = pl;

		// It's in kernel/generic
		assert_eq!(component.comp.as_ref(), "kernel");
		assert_eq!(component.subcomp.unwrap().as_ref(), "generic");

		// It's a Symlink
		use super::MetadataLine as ML;
		let f = match mdline {
			ML::SymLink(f) => f,
			_ => panic!("Shoulda been a SymLink: {:?}", mdline),
		};

		// igb -> em
		assert_eq!(f.path.to_str().unwrap(), "/boot/kernel/if_igb.ko");
		assert_eq!(f.target.to_str().unwrap(), "if_em.ko");

		// Expected owner etc.  Wacky that the symlinks are u+w...
		assert_eq!(f.uid, 0);
		assert_eq!(f.gid, 0);
		assert_eq!(f.mode, 0o755);
		assert_eq!(f.flags, 0);
	}


	#[test]
	fn reader()
	{
		let _inlines = r##"
world|base|/bin/[|f|0|0|0555|0|3ad985a50b79037b9672cf197fbc67bd54766199e190055101ea7d8c64ca843b|
world|base|/bin/test|f|0|0|0555|0|3ad985a50b79037b9672cf197fbc67bd54766199e190055101ea7d8c64ca843b|/bin/[
world|base|/var/empty|d|0|0|0555|400000||
kernel|generic|/boot/kernel/if_igb.ko|L|0|0|0755|0|if_em.ko|
"##;
		let mut inlines = _inlines.as_bytes();
		let parsed = super::parse_reader_lines(&mut inlines)
				.expect("Shoulda worked");
		assert_eq!(parsed.len(), 4, "got 4 rows");

		use super::MetadataLine as ML;

		match &parsed[0].mdline {
			ML::File(f) => assert_eq!(f.path.to_str().unwrap(), "/bin/["),
			x => panic!("parsed[0] shoulda been file:/bin/[: {x:?}"),
		};
		match &parsed[1].mdline {
			ML::HardLink(f) => assert_eq!(f.path.to_str().unwrap(), "/bin/test"),
			x => panic!("parsed[1] shoulda been link:/bin/test: {x:?}"),
		};
		match &parsed[2].mdline {
			ML::Dir(f) => assert_eq!(f.path.to_str().unwrap(), "/var/empty"),
			x => panic!("parsed[2] shoulda been dir:/var/empty: {x:?}"),
		};
		match &parsed[3].mdline {
			ML::SymLink(f) => assert_eq!(f.path.to_str().unwrap(),
					"/boot/kernel/if_igb.ko"),
			x => panic!("parsed[3] shoulda been symlink:/boot/kernel/if_igb.ko: {x:?}"),
		};
	}


	#[test]
	fn group()
	{
		let _inlines = r##"
kernel|generic-dbg|/usr/lib/debug/boot/kernel|d|0|0|0755|0||
kernel|generic-dbg|/usr/lib/debug/boot|d|0|0|0755|0||
kernel|generic-dbg|/usr/lib/debug|d|0|0|0755|0||
kernel|generic-dbg|/usr/lib|d|0|0|0755|0||
kernel|generic-dbg|/usr|d|0|0|0755|0||
kernel|generic-dbg|/|d|0|0|0755|0||
world|base|/usr/include/netinet/in_var.h|f|0|0|0444|0|aa977449131d51627be0600e59f9a48ade612e5822ebc9eda64f589a1ee6b925|
world|base|/usr/share/bhyve/kbdlayout/fr_dvorak_acc|f|0|0|0444|0|6eed475dfb1d6f4987034c7ca9a698d83300126b377f51dc5308e939ba6dbd78|
world|base|/sbin/poweroff|f|0|5|4554|0|d8ce3f6c3c5fbb2ad0ead6338749f450ad4d092086b695d498d1e0902a42e2c7|
world|base|/nonexistent|-||||||"
"##;

		// Do a linewise parse first, just for kicks
		let mut inlines = _inlines.as_bytes();
		let parsed = super::parse_reader_lines(&mut inlines)
				.expect("Shoulda worked");
		assert_eq!(parsed.len(), 10, "got 10 rows");

		// We've got 2 Components here, prebuild for easy comparison
		use crate::components::Component;
		let kcomp = "kernel/generic-dbg".parse().unwrap();
		let wcomp = "world/base".parse().unwrap();
		let comps: &[Component] = &[
			kcomp,
			wcomp,
		];

		// Just to double-check...
		let my_mdg: crate::metadata::MetadataGroup = parsed.into();

		// Parse out into MdG.  Redo it from scratch, just to test the
		// higher level
		let mut inlines = _inlines.as_bytes();
		let mdg = super::reader(&mut inlines)
				.expect("Shoulda worked");

		assert_eq!(mdg, my_mdg, "they're the same thing");

		// Should have those two components
		let mut keys: Vec<Component> = mdg.md.keys()
				.into_iter().map(|e| *e).collect();
		keys.sort();
		assert_eq!(keys, comps, "Got the right components");

		// Should have 6 dirs in the kernel bit and 3 files in the world
		assert_eq!(mdg.md[&kcomp].dirs.len(), 6,
				"right number of kernel dirs");
		assert_eq!(mdg.md[&wcomp].files.len(), 3,
				"right number of world files");

		// Test sorting
		use std::path::{PathBuf, Path};
		let dirs = [
			"/",
			"/usr",
			"/usr/lib",
			"/usr/lib/debug",
			"/usr/lib/debug/boot",
			"/usr/lib/debug/boot/kernel",
		];
		let dirs: Vec<PathBuf> = dirs.into_iter()
				.map(|d| d.into()).collect();
		let mut found_dirs: Vec<&Path> = mdg.md[&kcomp]
				.dirs.keys().map(|d| d.as_ref()).collect();
		found_dirs.sort_unstable();
		assert_eq!(dirs, found_dirs, "Got dirs sorted right");

		let files = [
			"/sbin/poweroff",
			"/usr/include/netinet/in_var.h",
			"/usr/share/bhyve/kbdlayout/fr_dvorak_acc",
		];
		let files: Vec<PathBuf> = files.into_iter()
				.map(|f| f.into()).collect();
		let mut found_files: Vec<&Path> = mdg.md[&wcomp]
				.files.keys().map(|f| f.as_ref()).collect();
		found_files.sort_unstable();
		assert_eq!(files, found_files, "Got files sorted right");

		// Also should have the one dash line in the world
		assert_eq!(mdg.md[&wcomp].dashes.len(), 1, "1 world dash line");
		let ne: &Path = "/nonexistent".as_ref();
		assert!(mdg.md[&wcomp].dashes.contains(ne));
	}
}
