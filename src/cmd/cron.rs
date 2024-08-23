//! $0 cron
use crate::command::CmdArg;

use anyhow::bail;


pub(crate) fn run(carg: CmdArg) -> Result<(), anyhow::Error>
{
	// Check our various config etc.
	check(&carg)?;


	/*
	 * Preload some bits.  Technically, cron doesn't need them, but they
	 * might need us to blow up early, so do that here.
	 */

	// Setting up various dirs
	let rtdirs = crate::core::RtDirs::init(&carg.config.basedir(),
			&carg.config.workdir())?;

	// See what sorta state we're in, and if it's one where we shouldn't
	// be running fetch.
	let state = rtdirs.state_load()?;

	// I'm gonna need to know my command name in a few places, so just
	// pre-figure it...
	let cmdname = crate::util::cmdname();

	// If we're showing kernel installed, that means there _is_ an
	// upgrade in progress, but not done (or there wouldn't be any
	// state), so...
	if state.upgrade_in_progress()
	{
		eprintln!("Partially completed upgrade already in progress.  \
				Perhaps you need to run `{cmdname} install` to finish.\n\
				Or run `{cmdname} clean --pending` to discard state.");
		bail!("upgrade in progress");
	}

	let crargs = match &carg.clargs.command {
		crate::command::FrCmds::Cron(c) => c,
		_ => unreachable!("I'm an upgrade, why does it think I'm not??"),
	};


	// OK, just thunk through.  In principal, we can probably setup some
	// args and call fetch::run() directly.  Doing that well requires
	// putting a lot more stuff in fetch to know about what/when to
	// output.  In practice, I'm just gonna re-exec ourself as fetch and
	// capture outputs.  That seems simpler.

	// Sleep somewhere in the arena of an hour
	if !crargs.immediately
	{
		use rand::{Rng, SeedableRng};
		let mut rng = rand_pcg::Pcg64::from_entropy();
		let sleep = rng.gen_range(0..3600) as u64;
		let dur = std::time::Duration::from_secs(sleep);
		std::thread::sleep(dur);
	}


	// Exec
	let myself = crate::util::argv_0().expect("Can't figure argv[0], bailing");
	let my_args = carg.clargs.mk_args();
	// XXX if fetch grows more args, we'll need to handle copying them
	// over here.

	// Exec and capture stdout.  We'll let stderr pass through, and let
	// cron email about errors.
	use std::process::{Command, Stdio};
	let mut cmd = Command::new(myself);
	cmd.args(my_args).arg("fetch").arg("--as-cron");   // + fetch args
	cmd.stderr(Stdio::inherit());
	let fout = cmd.output()?;

	// OK, get something valid-looking for the command output.
	let foutstr = String::from_utf8_lossy(&fout.stdout);

	// Should have exited cleanly.
	//
	// XXX Maybe I should make fetch --cron signal data with exit status
	// instead of what we're doing here...
	if !fout.status.success()
	{
		bail!("Running fetch failed: {:?}\n{foutstr}", fout.status);
	}

	// As with f-u.sh, do a definitely-reliable substring check to see if
	// it turned up something to do.
	// x-ref this output in fetch::run().
	let noup = "\nNo updates needed to update system to";
	if foutstr.contains(noup)
	{
		// Was nothing to do, exit cleanly.
		return Ok(())
	}

	// OK, that "no updated needed" wasn't in the output, so I guess
	// there's...  y'know.  Updates needed.
	let mailto = carg.config.mailto.as_deref().unwrap_or("root");
	let host = hostname::get()?;
	let host = host.to_string_lossy();
	let subj = format!("{host} security updates");
	let mut cmd = Command::new("/usr/bin/mail");
	cmd.args(["-s", &subj, mailto]).stdin(Stdio::piped());

	let mut mail = cmd.spawn()?;
	{
		use std::io::Write as _;
		let mut mailin = mail.stdin.take().expect("mail has stdin");
		mailin.write_all(&fout.stdout).expect("mail took the input");
		let vers = crate::VERSION;
		write!(mailin, "\n\n-- \n{cmdname} {vers}\n")?;
	}
	// and stdin should be closed.


	// Nothing left for us to do
	Ok(())
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
	check!(workdir);
	check!(basedir);

	// Should only run on releases (temporarily knocked off for dev)
	match crate::check::version(&carg.version) {
		Ok(_) => (),
		Err(_e) => (), // errs.push(e),
	};


	match errs.len() {
		0 => Ok(()),
		_ => {
			use anyhow::anyhow;
			let estr = anyhow!("Cannot run cron::\n  - {}",
					errs.join("\n  - "));
			Err(estr)
		},
	}
}
