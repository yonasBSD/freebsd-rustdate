//! $0 check-fetch
use crate::command::CmdArg;


pub(crate) fn run(carg: CmdArg) -> Result<u8, anyhow::Error>
{
	// Check our various config etc.
	check(&carg)?;

	// I'm gonna need to know my command name in a few places, so just
	// pre-figure it...
	//let cmdname = crate::util::cmdname();

	// OK, bust it up so we can move the bits around individually.
	let CmdArg { clargs, config, version } = carg;
	let args = match clargs.command {
		crate::command::FrCmds::CheckFetch(a) => a,
		_ => unreachable!("I'm a check-fetch, why does it think I'm not??"),
	};
	let quiet = args.quiet > 0;


	// Delay?
	if args.cron
	{
		use rand::{Rng, SeedableRng};
		let mut rng = rand_pcg::Pcg64::from_entropy();
		let sleep = rng.gen_range(0..3600) as u64;
		let dur = std::time::Duration::from_secs(sleep);
		std::thread::sleep(dur);
	}


	// Show our starting point
	if !quiet { println!("Currently running {version}."); }


	// Locate server to get the keytag stuff
	let server = crate::server::Server::find_inner(&config.servername,
			&version.kernel, &config.keyprint, quiet)?;

	// We're kinda fetch-y, so if we got something, it matches our
	// version; the only difference can be the patch.
	let my_version = version.max();
	let my_patch  = my_version.patch;
	let srv_patch = server.keytag_patchnum();

	// So are we up to date?
	if my_patch >= srv_patch
	{
		if !quiet { println!("Up to date."); }
		return Ok(0);
	}

	// We're not.  If quiet>1, we signal purely by exit code
	if args.quiet <= 1
	{
		let mut srv_version = my_version.clone();
		srv_version.patch = srv_patch;
		if !quiet { println!(""); }
		println!("\
				Running:    {my_version}\n\
				Server has: {srv_version}");
	}
	Ok(1)
}


/// Do some checks of our config/etc
fn check(carg: &CmdArg) -> Result<(), anyhow::Error>
{
	let mut errs: Vec<String> = vec![];

	macro_rules! check {
		( $fld:ident) => {
			match crate::check::$fld(&carg.config) {
				Ok(_) => (),
				Err(e) => errs.push(e),
			}
		};
	}

	// Lot of simple config fields that have common check types
	check!(servername);
	check!(keyprint);

	// Should only run on releases (temporarily knocked off for dev)
	match crate::check::version(&carg.version) {
		Ok(_) => (),
		Err(_e) => (), // errs.push(e),
	};


	match errs.len() {
		0 => Ok(()),
		_ => {
			use anyhow::anyhow;
			let estr = anyhow!("Cannot run check-fetch::\n  - {}",
					errs.join("\n  - "));
			Err(estr)
		},
	}
}
