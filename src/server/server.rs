//! Bits related to finding freebsd-update servers
use super::keytag::KeyTag;

// This also includes some grab-bag stuff that hasn't been split out, but
// maybe should, but I don't care enough to put the time into yet.

/// A single server entry.  This mostly will just be the necessary info
/// from the SRV lookup, and a flag or two for tracking our internal
/// state.
///
/// Note also that we're ignoring the port.  This is probably somewhat
/// wrong, but it's what the sh script is doing anyway, so we'll just
/// assume everything's http/80 until we know why to be less stupid.
#[derive(Debug, Default)]
#[derive(derivative::Derivative)]
// Ordering stuff is just for the sorting, so most of the fields don't
// matter (and some of them couldn't be compared anyway)
#[derivative(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Server
{
	/// SRV Priority
	pub(in crate::server) pri: u16,

	/// SRV Weight
	pub(in crate::server) weight: u16,

	/// The hostname
	pub(in crate::server) host: String,


	/// Various cached up bits for runtime
	#[derivative(PartialEq = "ignore")]
	#[derivative(PartialOrd = "ignore")]
	#[derivative(Ord = "ignore")]
	pub(in crate::server) cache: ServerCache,
}


/// Cached runtime info for a server
#[derive(Debug, Default)]
pub(in crate::server) struct ServerCache
{
	/// The base URL for doing requests from this server
	pub(in crate::server) burl: Option<url::Url>,

	/// A stashed up ureq agent
	pub(in crate::server) agent: Option<ureq::Agent>,

	/// The public key/tag info
	pub(in crate::server) keytag: Option<KeyTag>,

	/// The `files/` dir, where we put downloaded bits from the server
	/// (i.e., usually `/var/db/freebsd-update/files/`).
	pub(in crate::server) filesdir: Option<std::path::PathBuf>,
}


impl Server
{
	/// Find a valid server under a given name.
	pub(crate) fn find(name: &str, version: &crate::info::AVersion,
			keyprint: &str)
			-> Result<Server, anyhow::Error>
	{
		Self::find_inner(name, version, keyprint, false)
	}


	/// Inner impl of server finding
	pub(crate) fn find_inner(name: &str, version: &crate::info::AVersion,
			keyprint: &str, quiet: bool)
			-> Result<Server, anyhow::Error>
	{
		use std::io::{stdout, Write as _};

		// First, look up from that list
		let servers = super::lookup::servers(&name)?;

		// Find the first one that's useful.
		for mut srv in servers
		{
			if !quiet { print!("Trying server {}...", srv.name()); }
			stdout().flush()?;
			let sret = srv.get_key_tag(version, keyprint);
			match sret {
				Ok(_) => {
					if !quiet { println!("   OK."); }
					return Ok(srv);
				},
				Err(e) => {
					if !quiet { println!("\nFailed: {}", e); }
					// FALLTHRU
				},
			};
		}

		// Whelp, no, that's probably fatal...
		anyhow::bail!("Out of servers, giving up.");
	}

	pub(crate) fn name(&self) -> &str { &self.host }

	pub(crate) fn set_filesdir(&mut self, d: std::path::PathBuf)
	{ self.cache.filesdir = Some(d); }

	/// Show the patch number in our keytag
	pub(crate) fn keytag_patchnum(&self) -> Option<u32>
	{ self.cache.keytag.as_ref()?.patch }


	/// Build up EOL warning info, if applicable
	pub(crate) fn eol_warning(&self, vers: &crate::info::version::Version)
			-> Option<String>
	{
		// std at least can get us a good enough timestamp, but all the
		// formatting isn't gonna work well without chrono, so might as
		// well just work with it.
		use chrono::{DateTime, Local};
		let now = Local::now();

		// We should long ago have the keytag anywhere that calls this,
		// but if not...
		let eol_ts = self.cache.keytag.as_ref()?.eoltime;
		let eol = DateTime::from_timestamp(eol_ts, 0).unwrap();
		let eol: DateTime<Local> = eol.into();

		// Remainder encapsulated for easier testing
		eol_warning_be(now, eol, vers)
	}
}

// A lot of the cached into will be guaranteed to exist by the time
// we expect to use it.  Where "guaranteed" means "if the programmer
// didn't screw up".  And what're the chances of that?
//
// A Sufficiently Smart person could make this a procmacro on the
// ServerCache, but...
macro_rules! mk_cache_getter {
	( $fld:ident, $type:ty ) => {
		impl ServerCache
		{
			pub(crate) fn $fld(&self) -> Result<&$type, anyhow::Error>
			{
				use anyhow::anyhow;
				self.$fld.as_ref()
					.ok_or_else(|| anyhow!("Error: {} should exist: {:?}",
							stringify!($fld), self))
			}
		}
	};
}
mk_cache_getter!(burl,   url::Url);
mk_cache_getter!(agent,  ureq::Agent);
mk_cache_getter!(keytag, KeyTag);
mk_cache_getter!(filesdir, std::path::PathBuf);





/*
 * The rest of this is just internal implementation details of the
 * external entries above.
 */
fn eol_warning_be(now: chrono::DateTime<chrono::Local>,
		eol: chrono::DateTime<chrono::Local>,
		vers: &crate::info::version::Version)
		-> Option<String>
{
	// Has it alreaedy passed?  Then we definitely warn.
	if now >= eol
	{
		let rstr = format!("WARNING: {vers} HAS PASSED ITS END-OF-LIFE DATE.\n\
				Any security issues discovered after {eol}\n\
				will not have been corrected.");
		return Some(rstr);
	}


	// Is it in the next 3 months?  Then warn.  f-u.sh's idea of "3
	// months" is 91.25 days, I'm just gonna call it 90.
	//
	// Not curently planning to attempt f-u's "only warn every so
	// often" logic...
	let horizon = chrono::TimeDelta::try_days(90).unwrap();
	if now + horizon >= eol
	{
		// Fake up a variant of f-u.sh's "interval until"
		let until = eol - now;
		let udays = until.num_days();
		let ustr = if udays > 31 {
				let mons: i64 = udays / 31;
				let s = if mons > 1 { "s" } else { "" };
				format!("{mons} month{s}")
			}
			else if udays > 7 {
				let wks: i64 = udays / 7;
				let s = if wks > 1 { "s" } else { "" };
				format!("{wks} week{s}")
			}
			else {
				let s = if udays > 1 { "s" } else { "" };
				format!("{udays} day{s}")
			};

		let rstr = format!("WARNING: {vers} is approaching its \
				end-of-life date.\n\
				It is strongly recommended that you upgrade to a newer \
				release before\n{eol}  ({ustr}).");
		return Some(rstr);
	}


	// OK, well, shaddup then.
	None
}




#[cfg(test)]
pub(super) mod tests
{
	use super::*;

	pub(in crate::server) fn test_servers() -> Vec<Server>
	{
		vec![
			Server {
				host: "bob".to_string(),
				pri: 2,
				weight: 10,
				..Server::default()
			},
			Server {
				host: "jane".to_string(),
				pri: 3,
				weight: 40,
				..Server::default()
			},
			Server {
				host: "joe".to_string(),
				pri: 3,
				weight: 20,
				..Server::default()
			},
			Server {
				host: "barbara".to_string(),
				pri: 3,
				weight: 20,
				..Server::default()
			},
			Server {
				host: "slowpoke".to_string(),
				pri: 30,
				weight: 10,
				..Server::default()
			},
		]
	}

	#[test]
	fn sorting()
	{
		use crate::server::lookup::srvs_by_pri;
		let srvs = srvs_by_pri(test_servers());

		// Should be 3 entries, one for pri 2, 3, 30
		assert_eq!(srvs.len(), 3, "3 priorities");

		assert_eq!(srvs[0].len(), 1, "pri 2 len");
		assert_eq!(srvs[0][0].pri, 2, "pri 2");

		assert_eq!(srvs[1].len(), 3, "pri 3 len");
		assert_eq!(srvs[1][0].pri, 3, "pri 3");

		assert_eq!(srvs[2].len(), 1, "pri 30 len");
		assert_eq!(srvs[2][0].pri, 30, "pri 30");


		// pri 3 stuff should be sorted by weight then name
		assert_eq!(srvs[1][0].host, "barbara");
		assert_eq!(srvs[1][1].host, "joe");
		assert_eq!(srvs[1][2].host, "jane");
	}

	#[test]
	fn eolwarn()
	{
		use chrono::{DateTime, Local, Days};
		use super::eol_warning_be;

		// "Now" is flexible, right?
		let date = "2020-01-01T00:00:00Z";
		let now = DateTime::parse_from_rfc3339(date).unwrap();
		let now: DateTime<Local> = now.into();

		// So's a version...
		let vers = crate::info::version::fake("11.22-RELEASE").unwrap();

		// 30 days ahead/behind gives us a warn/error, 300 days ahead
		// gives us nothing.
		let d30  = Days::new(30);
		let d300 = Days::new(300);

		// So, 300 days ahead should yield nothing.
		let nnow = now.clone().into();
		let eol = now + d300;
		let ew = eol_warning_be(nnow, eol, &vers);
		assert!(ew.is_none());

		// 30 days ahead will warn quiet-ish
		let nnow = now.clone().into();
		let eol = now + d30;
		let ew = eol_warning_be(nnow, eol, &vers).expect("Should have a msg");
		assert!(ew.contains("end-of-life"));

		// 30 days behind will warn loud-ish
		let nnow = now.clone().into();
		let eol = now - d30;
		let ew = eol_warning_be(nnow, eol, &vers).expect("Should have a msg");
		assert!(ew.contains("END-OF-LIFE"));

	}
}
