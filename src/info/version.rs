//! Info about running system version

#[derive(Debug)]
pub(crate) struct Version
{
	pub(crate) kernel: AVersion,
	pub(crate) user: AVersion,
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct AVersion
{
	/// The release: "12.3", "14.0", etc.
	pub(crate) release: String,
	/// The release type: "RELEASE", "STABLE", "RC1", etc.
	pub(crate) reltype: String,
	/// The patch level: "12.3-RELEASE-p2" -> Some(2)
	pub(crate) patch: Option<u32>,
}


use std::fmt;
impl fmt::Display for AVersion
{
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
	{
		let vstr = mk_str(&self.release, &self.reltype, self.patch);
		write!(f, "{vstr}")
	}
}

impl fmt::Display for Version
{
	/// We presume whichever is 'higher' between kernel/user is our
	/// "current" version.
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
	{
		let max = self.max();
		fmt::Display::fmt(max, f)
	}
}


impl Version
{
	pub(crate) fn max(&self) -> &AVersion
	{
		std::cmp::max(&self.kernel, &self.user)
	}
}


/// Gen a string of a particular set of version info.  This is useful
/// because we apparently takes bits of versions from different places
/// during this process sometimes...
pub(crate) fn mk_str(rel: &str, rtype: &str, patch: Option<u32>) -> String
{
	let pstr = mk_patch_str(patch);
	format!("{rel}-{rtype}{pstr}")
}


/// Gen a string describing a patchlevel
pub(crate) fn mk_patch_str(patch: Option<u32>) -> String
{
	match patch {
		Some(p) => format!("-p{p}"),
		None => "".to_string(),
	}
}


/// Get info about currently installed release stuff, from
/// freebsd-version.
use std::path::Path;
pub(crate) fn get(bdir: &Path) -> Result<Version, anyhow::Error>
{
	let vout = run_freebsd_version(bdir)?;
	parse_freebsd_version(&vout)
}


/// Generate a fake version from a given string
pub(crate) fn fake(vstr: &str) -> Result<Version, anyhow::Error>
{
	let kernel = parse_version_row(vstr, "kernel")?;
	let user   = parse_version_row(vstr, "user")?;
	Ok(Version { kernel, user })
}


/// Get info about currently installed release stuff, from
/// freebsd-version.
/// Run freebsd-version to get kernel/user version info
fn run_freebsd_version(bdir: &Path) -> Result<Vec<u8>, anyhow::Error>
{
	use crate::util::path_join;
	let vcmd = path_join(bdir, "/bin/freebsd-version");
	let vout = std::process::Command::new(vcmd)
			.env("ROOT", bdir)
			.arg("-ku").output().map_err(|e| {
				anyhow::anyhow!("Running freebsd-version: {e}")
			})?;

	Ok(vout.stdout)
}

fn parse_freebsd_version(vout: &[u8]) -> Result<Version, anyhow::Error>
{
	use anyhow::bail;

	// Let's just assume it gave us 7-bit ASCII and we'll work on String's.
	let outlbufs = vout.split(|b| *b == b'\n');
	let outlstr: Result<Vec<&str>, _> = outlbufs
		.map(|l| std::str::from_utf8(l))
		.collect();
	let mut outlstr = match outlstr {
		Ok(v) => v,
		Err(e) => bail!("Error parsing version: {}", e),
	};
	// And skip the trailing blank
	outlstr.retain(|l| l.len() > 0);

	if outlstr.len() != 2
	{
		bail!("Expected 2 lines, not {}", outlstr.len());
	}

	// OK, first line is kernel, second is userland
	let kernel = parse_version_row(outlstr[0], "kernel")?;
	let user =   parse_version_row(outlstr[1], "user")?;

	Ok(Version { kernel, user })
}

fn parse_version_row(row: &str, rdesc: &str) -> Result<AVersion, anyhow::Error>
{
	row.parse().map_err(|e| anyhow::anyhow!("Error in {rdesc}: {e}"))
}

impl std::str::FromStr for AVersion
{
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err>
	{
		// <version>-p<patch>
		let mut riter = s.split("-p");
		let relbit = riter.next().ok_or("No release")?;
		let pat = riter.next();

		// <version> = 1.2-STABLE, etc
		let mut rsp = relbit.split("-");
		let release = rsp.next()
			.map(|s| s.to_string())
			.ok_or_else(|| format!("No version"))?;
		let reltype = rsp.next()
			.map(|s| s.to_string())
			.ok_or_else(|| format!("No version type"))?;

		let patch = match pat {
			None => None,
			Some(s) => {
				let pnum = s.parse::<u32>()
					.map_err(|e| format!("Bad patch version: {}", e))?;
				Some(pnum)
			},
		};
		Ok(AVersion { release, reltype, patch })
	}
}


#[cfg(test)]
mod tests
{
	use super::*;

	#[test]
	fn version()
	{
		// Basic test
		let vout = br##"
12.3-STABLE
12.3-RELEASE-p2
"##;
		let vers = parse_freebsd_version(vout).unwrap();
		assert_eq!(vers.kernel.release, "12.3");
		assert_eq!(vers.kernel.reltype, "STABLE");
		assert_eq!(vers.kernel.patch, None);
		assert_eq!(vers.user.release, "12.3");
		assert_eq!(vers.user.reltype, "RELEASE");
		assert_eq!(vers.user.patch, Some(2));
	}

	#[test]
	fn fake_version()
	{
		// Test a fake one
		let fver = "1.2.3-RC1-p1";
		let vers = super::fake(fver).unwrap();
		assert_eq!(vers.kernel.release, "1.2.3");
		assert_eq!(vers.user.release,   "1.2.3");

		assert_eq!(vers.kernel.reltype, "RC1");
		assert_eq!(vers.user.reltype,   "RC1");

		assert_eq!(vers.kernel.patch, Some(1));
		assert_eq!(vers.user.patch,   Some(1));
	}

	#[test]
	fn display_version()
	{
		let vout = br##"
12.3-RELEASE
12.3-RELEASE-p2
"##;
		let mut vers = parse_freebsd_version(vout).unwrap();
		assert_eq!(vers.to_string(), "12.3-RELEASE-p2");
		vers.kernel.patch = Some(3);
		assert_eq!(vers.to_string(), "12.3-RELEASE-p3");
	}
}
