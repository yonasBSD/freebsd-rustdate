//! Fetching a set of hashfiles
use crate::core::pool::hashcheck as hcp;
use crate::util::hash;
use crate::server::Server;



/// Given a set of hashes and a prebuilt Control with info about where
/// things get stashed along the way, fetch the files, check the hashes,
/// and store them permanently.
///
/// Any returned error is probably fatal; something broke, or we didn't
/// get them all, and we _should_ get them all...
pub(crate) fn get(srv: &Server, hashes: Vec<hash::Sha256HashBuf>,
		ctrl: hcp::Control) -> Result<(), anyhow::Error>
{
	// We need the list of hashnames, not just the hashes.
	println!("Fetching {} new files.", hashes.len());
	let fnames: Vec<String> = hashes.iter()
			.map(|f| format!("{f}.gz")).collect();
	let nf = srv.fetch_files(fnames.clone(), ctrl.tmpdir.clone())?;
	assert_eq!(nf as usize, fnames.len());

	// OK, we presumably got 'em all.  Check the hashes and store
	// into files/.
	// Wrap up the paths in the request struct
	let reqs: Vec<_> = fnames.into_iter()
			.map(|path| hcp::Req { path }).collect();
	let rlen = reqs.len();
	println!("Checking {} hashes.", rlen);

	// Do the pool's work
	let hcres = {
		use crate::core::pool::Pool as _;
		let sp = hcp::HashCheck::new(rlen);
		sp.run(&ctrl, reqs)?
	};

	// Any errors probably means we failed.
	if let Some(errs) = hcres.errs
	{
		return Err(errs)?;
	}

	// If there weren't errs, this better even out...
	let oklen = hcres.oks.len();
	if oklen != rlen
	{
		anyhow::bail!("Internal errr: should have checked {rlen}, \
				but only got {oklen}.");
	}

	Ok(())
}
