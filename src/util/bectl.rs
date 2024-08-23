//! bectl(1) handling for boot environments.

static BECTL: &str = "/sbin/bectl";


/// Check: are boot envs supported?
pub(crate) fn enabled() -> Result<bool, anyhow::Error>
{
	// XXX Should we be running the <basedir>/bectl instead of <running
	// system>/bectl?  Since we don't really support a subpath, and
	// f-u.sh just flat out doesn't even try BE's with a basedir, I guess
	// we'll skip right over it...  higher levels will make that kinda
	// decision.

	// If we're jailed, we don't.
	if crate::info::kernel::jailed()?
	{ return Ok(false); }

	// OK, see what bectl thinks.
	let bret = std::process::Command::new(BECTL)
			.arg("check").status()?;
	Ok(bret.success())
}


/// Try amking a BE with a given snapshot name.
pub(crate) fn create(name: &str) -> Result<(), anyhow::Error>
{
	let mut bcmd = std::process::Command::new(BECTL);
	bcmd.args(["create", "-r", name]);

	let bret = bcmd.output()?;

	if !bret.status.success()
	{ anyhow::bail!("bectl failed: {:?}", bret); }

	Ok(())
}
