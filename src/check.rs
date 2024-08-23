//! Various runtime checks of things
use crate::config::Config;


/*
 * Many of these are simple "This is OK, or we know an error"
 */
pub(crate) fn servername(conf: &Config) -> Result<(), String>
{
	match conf.servername.len() {
		0 => Err("No server name given".to_string()),
		_ => Ok(()),
	}
}

pub(crate) fn keyprint(conf: &Config) -> Result<(), String>
{
	match conf.keyprint.len() {
		0 => Err("No key fingerprint given".to_string()),
		64 => {
			match conf.keyprint.chars().any(|c| !c.is_ascii_hexdigit()) {
				true => Err("Invalid KeyPrint given in config file \
							(not hex string)".to_string()),
				false => Ok(()),
			}
		},
		_ => Err("Invalid KeyPrint given in config file (bad length)".to_string()),
	}
}

pub(crate) fn workdir(conf: &Config) -> Result<(), String>
{
	let wd = conf.workdir();

	if !wd.is_dir()
	{
		Err(format!("No such working directory {}", wd.display()))?
	}

	// libc::access is a thing, but it's a little gross...   assume we're
	// root for now and worry about when we're not later.

	Ok(())
}

pub(crate) fn basedir(conf: &Config) -> Result<(), String>
{
	let bd = conf.basedir();

	if !bd.is_dir()
	{
		Err(format!("No such base directory {}", bd.display()))?
	}

	// x-ref workdir() about perms

	Ok(())
}



/*
 * There's a version check in freebsd-update.sh, that won't let you run
 * if it we're not a -RELEASE or near-RELEASE.  Things are a little hinky
 * since it's going based on the running kernel version from uname, and
 * boy is there a lot of uncertainty about details, but...  we'll do as
 * well as we can, by pretending from the kernel version we got from
 * freebsd-version.
 */
pub(crate) fn version(vers: &crate::info::Version) -> Result<(), String>
{
	let rstr = r##"
Cannot upgrade from a version that is not a release
(including alpha, beta and release candidates).
"##;
	match vers.kernel.reltype.as_ref() {
		// Common case
		"RELEASE" => Ok(()),
		s if s.starts_with("ALPHA") => Ok(()),
		s if s.starts_with("BETA")  => Ok(()),
		s if s.starts_with("RC")    => Ok(()),
		_ => Err(rstr.to_string()),
	}
}
