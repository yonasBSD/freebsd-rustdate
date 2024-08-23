//! Loading and dealing with freebsd-update.conf and runtime usage of its
//! bits.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use thiserror::Error;
use regex_lite::Regex;

use crate::components::Component;


#[derive(Debug)]
#[derive(derivative::Derivative)]
#[derivative(Default)]
pub struct Config
{
	/// Trusted keyprint
	pub(crate) keyprint: String,

	/// Server to fetch updates from
	// XXX This type seems like it could be better, but
	// hickory_resolver::Name From's String types, so...
	pub(crate) servername: String,

	/// Components to update
	pub(crate) components: HashSet<Component>,

	/// Paths to Ignore.  According to the docs, these are prefix matches
	/// (path component prefix?  Intra-file/path-name prefix?).  In
	/// implementation, they're anchored `grep -E` matches, so we'll go
	/// ahead and treat 'em like regexes I guess...
	pub(crate) ignore_paths: Vec<Regex>,

	/// Paths to Ignore in IDS mode.  For us, that's check-sys.
	pub(crate) ids_ignore_paths: Vec<Regex>,

	/// Paths to update only if the user hasn't changed them from our
	/// best guess at the upstream contents.
	pub(crate) update_if_unmodified: Vec<Regex>,

	/// Merge changes to matching files
	pub(crate) merge_changes: Vec<Regex>,

	/// Keep modifications to metadata (perms, owner, flags, etc)
	#[derivative(Default(value="true"))]
	pub(crate) keep_modified_metadata: bool,

	/// Notification email address for `cron` command.
	pub(crate) mailto: Option<String>,


	/// What dir we're working from
	#[derivative(Default(value="\"/\".into()"))]
	basedir: PathBuf,

	/// Where updates and temporary files are stored
	#[derivative(Default(value="\"/var/db/freebsd-update\".into()"))]
	workdir: PathBuf,

	/// Create a new boot environment when installing patches
	#[derivative(Default(value="true"))]
	pub(crate) create_boot_env: bool,

	/// Alternate root database for boot environments.  This is required
	/// to set boot environments when the basedir is not `/`.
	///
	/// XXX This is just an idea, and isn't tested or supported.
	pub(crate) boot_env_root: Option<String>,
}


impl Config
{
	// Some fields kept hidden so we can make sure they don't change from
	// under us, so we can cache derived bits.
	pub(crate) fn basedir(&self) -> &std::path::Path { &self.basedir }
	pub(crate) fn workdir(&self) -> &std::path::Path { &self.workdir }


	/// "Finalize" components.  This is a kinda one-off hack to remove
	/// the src component if the system doesn't seem to have src
	/// installed.  And it doesn't seem to consider "src/src" if that's
	/// given explicitly either.  A little hinky, but hey...
	pub(crate) fn finalize_components(&mut self)
	{
		// So if src _is_ apparently there, there's nothing to do
		let checkfile = self.basedir.join("usr/src/COPYRIGHT");
		if checkfile.is_file() { return; }
		let src_comp = "src".parse().unwrap();
		self.components.retain(|c| c != &src_comp);
	}
}


/// Problems loading config
#[derive(Debug)]
#[derive(Error)]
pub enum ConfigErr
{
	/// File I/O error of some sort
	#[error("Config file I/O error: {0}")]
	IO(#[from] std::io::Error),

	/// Syntax error in the config file
	#[error("Config file syntax error: {0}")]
	Syntax(String),

	/// Explicitly unsupported config params, because I have limited
	/// scope.
	#[error("Unsupported config: {0}")]
	Unsupported(String),
}



/// Load in the config, with appropriate overrides from command-line args
pub(crate) fn load_config_file(cfile: &Path, clargs: &crate::command::FrArgs)
		-> Result<Config, ConfigErr>
{
	// Load from the file
	let conf = std::fs::read(cfile)?;
	load_config(&conf, clargs)
}


/// Parse the config, with appropriate overrides from command-line args
pub(crate) fn load_config(conf: &[u8], clargs: &crate::command::FrArgs)
		-> Result<Config, ConfigErr>
{
	// Load from the file
	let mut conf = load(&conf)?;

	// And override from CL args as appropriate
	macro_rules! or {
		( $fld:ident ) => {
			conf.$fld = match &clargs.$fld {
				Some(x) => x.clone(),
				None    => conf.$fld,
			};
		};
		( $cfld:ident, $afld:ident ) => {
			conf.$cfld = match &clargs.$afld {
				Some(x) => x.clone(),
				None    => conf.$cfld,
			};
		};
	}
	or!(basedir);
	or!(workdir);
	or!(servername);


	Ok(conf)
}


// /// Parse out config from a file
// fn load_file(cfile: &Path) -> Result<Config, ConfigErr>
// {
// 	let conf = std::fs::read(cfile)?;
// 	load(&conf)
// }


/// Parse out a string of the config
fn load(conf: &[u8]) -> Result<Config, ConfigErr>
{
	let mut config = Config::default();

	for inline in conf.split(|c| *c == b'\n')
	{
		// Discard any parts past a comment
		let line = match inline.splitn(2, |c| *c == b'#').next() {
			Some(l) => l,
			None => continue,
		};

		// Split out into [param, value]; lines not matching that aren't
		// useful config.
		let [par, val] = {
			let mut it = line.splitn(2, |c| *c == b' ');
			let par = it.next();
			let val = it.next();
			match (par, val) {
				(Some(p), Some(v)) => [p, v],
				(_, _) => continue,
			}
		};

		// Some of the [u8] -> X conversions we use
		let stringify = |bytes, ewhat| -> Result<String, ConfigErr> {
			Ok(std::str::from_utf8(bytes).map_err(|e| {
				ConfigErr::Syntax(format!("Error parsing {ewhat}: {e}"))
			})?.into())
		};
		let pathify = |bytes: &[u8]| -> PathBuf {
			let pvec = bytes.to_vec();
			use std::os::unix::ffi::OsStringExt;
			let pstr = OsString::from_vec(pvec);
			let npath = PathBuf::from(pstr);
			npath
		};
		let regexify = |bytes: &[u8], ewhat| -> Result<Regex, ConfigErr> {
			// Regex reallys wants str, so we convert.  Also, this is
			// only used for IgnorePaths, which is documented as being
			// anchored to start.
			let str = std::str::from_utf8(bytes).map_err(|e| {
				ConfigErr::Syntax(format!("Error stringifying {ewhat}: {e}"))
			})?;
			let str = format!("^{str}");
			let re = Regex::new(&str).map_err(|e| {
				ConfigErr::Syntax(format!("Error building regex from {ewhat}: {e}"))
			})?;
			Ok(re)
		};
		let boolify = |bytes: &[u8]| -> Option<bool> {
			// sh script allows [Yy][Ee][Ss] etc.  I don't wanna bother
			// unless I must
			Some(match bytes {
				b"yes" => true,
				b"no"  => false,
				_      => None?,
			})
		};

		// Now let's see what params and vals we're messing with
		match par
		{
			b"KeyPrint" => config.keyprint = stringify(val, "KeyPrint")?,
			b"ServerName" => config.servername = stringify(val, "ServerName")?,
			b"Components" => {
				for comp in val.split(|c| *c == b' ')
				{
					if comp.len() == 0 { continue }
					let cstr = stringify(comp, "Component")?;
					let comp: Component = cstr.parse()
							.map_err(|e| ConfigErr::Syntax(e))?;
					config.components.insert(comp);
				}
			},
			b"IgnorePaths" => {
				for path in val.split(|c| *c == b' ')
				{
					if path.len() == 0 { continue }
					// x-ref note in regexify()
					config.ignore_paths.push(regexify(path, "IgnorePaths")?);
				}
			},
			b"IDSIgnorePaths" => {
				for path in val.split(|c| *c == b' ')
				{
					if path.len() == 0 { continue }
					// x-ref note in regexify()
					config.ids_ignore_paths.push(regexify(path, "IgnorePaths")?);
				}
			},
			b"UpdateIfUnmodified" => {
				for path in val.split(|c| *c == b' ')
				{
					if path.len() == 0 { continue }
					// x-ref note in regexify()
					config.update_if_unmodified.push(
							regexify(path, "UpdateIfUnmodified")?);
				}
			},
			b"MergeChanges" => {
				for path in val.split(|c| *c == b' ')
				{
					if path.len() == 0 { continue }
					config.merge_changes.push(regexify(path, "MergeChanges")?);
				}
			},
			b"BaseDir" => {
				if val.len() == 0 { continue }
				config.basedir = pathify(val);
			},
			b"WorkDir" => {
				if val.len() == 0 { continue }
				config.workdir = pathify(val);
			},
			b"CreateBootEnv" => {
				config.create_boot_env = boolify(val).ok_or_else(|| {
					ConfigErr::Syntax(format!("Bad CreateBootEnv value {}",
						String::from_utf8_lossy(val)))
				})?;
			},
			b"BootEnvRoot" => {
				if val.len() == 0 { continue }
				eprintln!("BootEnvRoot doesn't do anything...");
				config.boot_env_root = Some(stringify(val, "BootEnvRoot")?);
			},
			b"KeepModifiedMetadata" => {
				config.keep_modified_metadata  = boolify(val).ok_or_else(|| {
					ConfigErr::Syntax(format!("Bad KeepModifiedMetadata value {}",
						String::from_utf8_lossy(val)))
				})?;
			},
			b"MailTo" => {
				config.mailto = Some(stringify(val, "MailTo")?)
			},

			// Explicitly call out some things I'm intentionally skipping
			// support of for now.
			b"AllowAdd" => {
				match boolify(val)
				{
					Some(v) if !v => {
						let estr = format!("AllowAdd=no");
						return Err(ConfigErr::Unsupported(estr));
					}
					_ => (),
				}
			},
			b"AllowDelete" => {
				match boolify(val)
				{
					Some(v) if !v => {
						let estr = format!("AllowDelete=no");
						return Err(ConfigErr::Unsupported(estr));
					}
					_ => (),
				}
			},

			_ => continue,
		};
	}


	Ok(config)
}




#[cfg(test)]
mod tests
{
	use super::{load, load_config};

	// committer	Warner Losh <imp@FreeBSD.org>	2023-08-16 17:55:03 +0000
	// commit	d0b2dbfa0ecf2bbc9709efc5e20baf8e4b44bbbf
	const DEFCONF: &[u8] = br##"
# Trusted keyprint.  Changing this is a Bad Idea unless you've received
# a PGP-signed email from <security-officer@FreeBSD.org> telling you to
# change it and explaining why.
KeyPrint 800651ef4b4c71c27e60786d7b487188970f4b4169cc055784e21eb71d410cc5

# Server or server pool from which to fetch updates.  You can change
# this to point at a specific server if you want, but in most cases
# using a "nearby" server won't provide a measurable improvement in
# performance.
ServerName update.FreeBSD.org

# Components of the base system which should be kept updated.
Components src world kernel

# Example for updating the userland and the kernel source code only:
# Components src/base src/sys world

# Paths which start with anything matching an entry in an IgnorePaths
# statement will be ignored.
IgnorePaths /foo/bar

# Paths which start with anything matching an entry in an IDSIgnorePaths
# statement will be ignored by "freebsd-update IDS".
IDSIgnorePaths /usr/share/man/cat
IDSIgnorePaths /usr/share/man/whatis
IDSIgnorePaths /var/db/locate.database
IDSIgnorePaths /var/log

# Paths which start with anything matching an entry in an UpdateIfUnmodified
# statement will only be updated if the contents of the file have not been
# modified by the user (unless changes are merged; see below).
UpdateIfUnmodified /etc/ /var/ /root/ /.cshrc /.profile

# When upgrading to a new FreeBSD release, files which match MergeChanges
# will have any local changes merged into the version from the new release.
MergeChanges /etc/ /boot/device.hints

### Default configuration options:

# Directory in which to store downloaded updates and temporary
# files used by FreeBSD Update.
# WorkDir /var/db/freebsd-update

# Destination to send output of "freebsd-update cron" if an error
# occurs or updates have been downloaded.
# MailTo root

# Is FreeBSD Update allowed to create new files?
# AllowAdd yes

# Is FreeBSD Update allowed to delete files?
# AllowDelete yes

# If the user has modified file ownership, permissions, or flags, should
# FreeBSD Update retain this modified metadata when installing a new version
# of that file?
# KeepModifiedMetadata yes

# When upgrading between releases, should the list of Components be
# read strictly (StrictComponents yes) or merely as a list of components
# which *might* be installed of which FreeBSD Update should figure out
# which actually are installed and upgrade those (StrictComponents no)?
# StrictComponents no

# When installing a new kernel perform a backup of the old one first
# so it is possible to boot the old kernel in case of problems.
# BackupKernel yes

# If BackupKernel is enabled, the backup kernel is saved to this
# directory.
# BackupKernelDir /boot/kernel.old

# When backing up a kernel also back up debug symbol files?
# BackupKernelSymbolFiles no

# Create a new boot environment when installing patches
# CreateBootEnv yes
"##;

	#[test]
	fn default_parse()
	{
		let conf = load(DEFCONF).unwrap();

		let kp = "800651ef4b4c71c27e60786d7b487188970f4b4169cc055784e21eb71d410cc5";
		assert_eq!(conf.keyprint, kp);

		assert_eq!(conf.servername, "update.FreeBSD.org");

		let expected = ["src", "world", "kernel"].into_iter()
				.map(|c| c.parse().unwrap()).collect();
		assert_eq!(conf.components, expected);

		let mcc: &[String] = &[
			"^/etc/".into(),
			"^/boot/device.hints".into()
		];
		let mcstr: Vec<_> = conf.merge_changes.iter().map(|r| r.to_string())
				.collect();
		assert_eq!(conf.merge_changes.len(), 2, "2 MergeChanges");
		assert_eq!(mcstr, mcc);

		assert_eq!(conf.ignore_paths.len(), 1, "1 IgnorePaths");
		assert_eq!(conf.ignore_paths[0].to_string(), "^/foo/bar");

		assert_eq!(conf.update_if_unmodified.len(), 5, "5 UpIfUn's");
		assert_eq!(conf.update_if_unmodified[2].to_string(), "^/root/");

		// Default values
		use std::ffi::OsStr;
		assert_eq!(conf.basedir, OsStr::new("/"));
		assert_eq!(conf.workdir, OsStr::new("/var/db/freebsd-update"));
		assert_eq!(conf.create_boot_env, true);
		assert_eq!(conf.mailto, None);
	}

	#[test]
	fn component()
	{
		// Be sure component and subcomponent bits get parsed right
		let conf = b"Components src kernel/generic-dbg";
		let conf = load(conf).unwrap();

		let mut cstrs: Vec<String> = conf.components.iter()
				.map(|c| c.to_string()).collect();
		cstrs.sort_unstable();
		assert_eq!(cstrs.len(), 2, "2 components");
		assert_eq!(cstrs[0], "kernel/generic-dbg", "First is generic-dbg");
		assert_eq!(cstrs[1], "src", "Second is src");
	}

	#[test]
	fn workdir()
	{
		use std::ffi::OsStr;

		// Start with nothing, so we get default
		let cstr = b"";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.workdir, OsStr::new("/var/db/freebsd-update"));

		// Now set it to something
		let cstr = b"WorkDir /foo/bar";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.workdir, OsStr::new("/foo/bar"));
	}

	#[test]
	fn bootenv()
	{
		// Start with nothing, so we get default
		let cstr = b"";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.create_boot_env, true);

		// Now set it to something
		let cstr = b"CreateBootEnv no";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.create_boot_env, false);
	}

	#[test]
	fn bootenv_root()
	{
		// Start with nothing, so we get default
		let cstr = b"";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.boot_env_root, None);

		// Now set it to something
		let cstr = b"BootEnvRoot as/df";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.boot_env_root, Some("as/df".to_string()));
	}

	#[test]
	fn keepmodifiedmetadata()
	{
		// Start with nothing, so we get default
		let cstr = b"";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.keep_modified_metadata, true);

		// Now set it to something
		let cstr = b"KeepModifiedMetadata no";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.keep_modified_metadata, false);
	}


	#[test]
	fn mailto()
	{
		// Start with nothing, so we get default
		let cstr = b"";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.mailto, None);

		// Now set it to something
		let cstr = b"MailTo bob@your.uncle";
		let conf = load(cstr).unwrap();
		assert_eq!(conf.mailto, Some("bob@your.uncle".to_string()));
	}


	fn make_fake_clargs() -> crate::command::FrArgs
	{
		crate::command::FrArgs::default()
	}

	#[test]
	fn cli_override()
	{
		let mut args = make_fake_clargs();

		// Plain config
		let conf = load_config(DEFCONF, &args).unwrap();
		assert_eq!(conf.servername, "update.FreeBSD.org");

		// With override
		let dg = "downgrade.fREEbsd.org";
		args.servername = Some(dg.to_string());
		let conf = load_config(DEFCONF, &args).unwrap();
		assert_eq!(conf.servername, dg);
	}

	#[test]
	fn allow_add()
	{
		let conf = b"AllowAdd yes";
		load(conf).expect("AllowAdd yes ok");
		let conf = b"AllowAdd no";
		load(conf).expect_err("AllowAdd no not ok");
	}

	#[test]
	fn allow_delete()
	{
		let conf = b"AllowDelete yes";
		load(conf).expect("AllowDelete yes ok");
		let conf = b"AllowDelete no";
		load(conf).expect_err("AllowDelete no not ok");
	}
}
