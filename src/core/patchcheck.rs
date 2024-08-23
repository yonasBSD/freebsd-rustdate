//! Apply a bunch of patches, check the results, and stick 'em in
//! filesdir.
use crate::core::pool::hashcheck as hcp;
use crate::core::pool::patch as pp;



/// Given a set of patches and a Control with info about dirs, apply the
/// patches to create the output files, check the hashes, and store
/// compressed variants.  This does a double-step of the patching-pool,
/// followed by the hashcheck-pool.
///
/// On success, returns a Vec<> of the output hashes that succeeded.
/// That means there should be a <filesdir>/<hash>.gz for each of them,
/// and maybe a <tmpdir>/<hash> if keep=true.  The patchfile themselves
/// are untouched.
///
/// A returned error is presumably fatal, meaning something blew up
/// unexpectedly.  However, it's not necessarily broken if we don't
/// successfully apply and create each patch; these patches are only ever
/// an optimization to avoid downloading the whole outfile anyway, so an
/// Err() result only means "damn, we blew up hard", not "well, we didn't
/// succeeed at everything we tried".
pub(crate) fn patch(patches: Vec<String>, ctrl: pp::Control)
		-> Result<Vec<String>, anyhow::Error>
{
	use crate::core::pool::Pool as _;

	// OK, first, we apply all the patches.
	let preqs: Vec<_> = patches.into_iter()
			.map(|patch| pp::Req{patch}).collect();
	let prlen = preqs.len();
	println!("Trying to apply {prlen} patches.");
	let patchres = {
		let pp = pp::Patch::new(prlen);
		pp.run(&ctrl, preqs)?
	};

	// Now, any errors, we pretty much ignore.  Any successes are now
	// <outhash>'s that exist as files in <tmpdir>.
	let pp::PoolResult { oks, errs } = patchres;
	let _ = errs;  // explicitly ignore
	let oklen = oks.len();
	println!("{oklen} successful.");
	if oklen == 0 { return Ok(Vec::new()); }

	// Check the hashes, and compress them into <filesdir> if the match.
	let hcreqs: Vec<_> = oks.into_iter().map(|res| {
			let path = format!("{}.gz", res.hash);
			hcp::Req { path }
		}).collect();
	println!("Checking hashes.");
	let hcres = {
		let sp = hcp::HashCheck::new(oklen);
		let ctrl = ctrl.into();
		sp.run(&ctrl, hcreqs)?
	};
	let hcp::PoolResult { oks, errs: _ } = hcres;
	let oklen = oks.len();
	println!("{oklen} successful.");
	if oklen == 0 { return Ok(Vec::new()); }

	// Translate the successes and return.
	let ret = oks.into_iter().map(|r| r.hash).collect();
	Ok(ret)
}
