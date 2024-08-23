//! Various http stuff.
use super::Server;
use std::path::PathBuf;

use url::Url;


impl Server
{
	/*
	 * External funcs
	 */

	/// Fetch a set of patch files into a dir (presumably a temp dir).
	/// This is used during fetching to try and get patches instead of
	/// whole files.
	///
	/// Returns the list of successfully fetched patches, or an error.
	/// Error is generally presumptively fatal; things like missing files
	/// or failed requests aren't "errors" in this meaning, they're just
	/// "whelp, didn't find that patch".
	pub(crate) fn fetch_patch_files(&self, patches: Vec<String>,
			tmpdir: PathBuf) -> Result<Vec<String>, anyhow::Error>
	{
		// Setup the fetch pool
		use crate::core::pool::fetch;
		let fp = fetch::Fetch::new(patches.len());

		let agent = self.cache.agent()?.clone();
		// XXX patchdir differs for upgrade?  Tackle this then...
		let baseurl = self.cache.burl()?.join("bp/")?;
		let path = tmpdir;
		let ctrl = fetch::Control { agent, baseurl, path };

		// Prep up requests
		let reqs = patches.into_iter()
				.map(|file| fetch::Req { file })
				.collect();

		// Run it
		let fres = {
			use crate::core::pool::Pool as _;
			fp.run(&ctrl, reqs)?
		};

		// We only care about what succeeded; any errors in the process
		// don't matter to us.
		Ok(fres.okfiles)
	}


	/// Fetch complete files into a (presumable temp)dir.  This is so we
	/// can split up the "fetch" and "check hash" steps, since we may
	/// want different levels of parallelism for them.
	///
	/// We expect and assume this will complete completely; if we can't
	/// get all the requested files, this is probably fatal.  Returns the
	/// number of fetched files on success, which probably doesn't mean
	/// much...
	pub(crate) fn fetch_files(&self, files: Vec<String>,
			tmpdir: PathBuf) -> Result<u32, anyhow::Error>
	{
		// Complete files are under <baseurl>/f
		let url = self.cache.burl()?.join("f/")?;
		self.fetch_files_from_to(url, files, tmpdir)
	}




	/*
	 * Higher-level calling funcs
	 */

	/// Fetch a set of files from a base URL into a dir.  Mostly a
	/// backend-sorta thing that we build more special-purpose frontends
	/// onto.
	///
	/// This very much expects to succeed at getting every file, and if
	/// it doesn't, that's an error.  That's simpler for cases where we
	/// require that result, but not so suitable for cases where we're OK
	/// with not finding everything we're trying.
	pub(super) fn fetch_files_from_to(&self, baseurl: Url,
			files: Vec<String>, path: PathBuf)
			-> Result<u32, anyhow::Error>
	{
		let agent = self.cache.agent()?.clone();

		// Setup a fetching pool
		use crate::core::pool::fetch;
		let fp = fetch::Fetch::new(files.len());
		let ctrl = fetch::Control { agent, baseurl, path };

		// Build up the individual requests
		let reqs = files.into_iter()
				.map(|file| fetch::Req { file })
				.collect();

		// And run it
		let fres = {
			use crate::core::pool::Pool as _;
			fp.run(&ctrl, reqs)?
		};


		// Figure out smarter returns sometime.  For now, if there are
		// errors, just return them, else the file count.
		if let Some(errs) = fres.errs { return Err(errs)?; }
		if fres.nfiles as usize != fres.okfiles.len()
		{
			anyhow::bail!("Expected {} files, fetched {}",
					fres.nfiles, fres.okfiles.len());
		}

		Ok(fres.nfiles)
	}


	/*
	 * The lower-level actual fetching bits.
	 */

	/// Do a GET and dump the results into a Vec<u8>.  I guess there's some
	/// way to do this via ureq's http-interop feature and http::Response?
	/// But instead, we're doing this whole thing manually for the moment...
	///
	/// This is intended as a simple util for fetching "small" files (up to a
	/// few dozen k, maybe), that we're just going to be poking through for
	/// stuff.  It's not built for fetching big files, or saving the results
	/// out to disk; that'll go elsewhere.
	pub(in crate::server) fn get_bytes(&self, url: &url::Url)
			-> Result<Vec<u8>, anyhow::Error>
	{
		let agent = self.cache.agent()?.clone();
		get_bytes(&agent, url)
	}
}



/// Backend for Server::get_bytes().
///
/// This is kept separate so Server::get_key_tag() can call it directly
/// with an agent, because that's still in the process of choosing a
/// Server to use, so it would be annoying to potentially make extra
/// Agent's that hang around on those Servers we didn't choose without
/// being dropped until <some arbitrary time>.
///
/// This way we can do those checks _before_ deciding "we'll keep this
/// server" and holding that Agent around for the rest of the process.
pub(in crate::server) fn get_bytes(agent: &ureq::Agent, url: &url::Url)
		-> Result<Vec<u8>, anyhow::Error>
{
	// These are small files to directly poke at, so set a limit big
	// enough to easily fit anything we expect, but not blow out memory
	// if somebody messes with us.
	const LIMIT: u64 = 10 * 1024 * 1024;

	let resp = agent.request_url("GET", &url)
		.call()?;
	let clen: Option<usize> = match resp.header("Content-Length") {
		Some(len) => match len.parse() {
			Ok(cl) => Some(cl),
			Err(_) => None,
		},
		None => None,
	};
	let mut data: Vec<u8> = match clen {
		Some(b) => Vec::with_capacity(b),
		None    => Vec::new(),
	};

	use std::io::Read;
	resp.into_reader().take(LIMIT).read_to_end(&mut data)?;
	Ok(data)
}



/// Creating an Agent for our use.  Centralize to make later adjustments
/// a little easier...
pub(in crate::server) fn mk_agent() -> ureq::Agent
{
	use std::time::Duration;

	ureq::AgentBuilder::new()
		.timeout_connect(Duration::from_secs(10))
		.timeout_read(Duration::from_secs(10))
		.build()
}
