//! Post-<step> bits.
//!
//! This is mostly running utils to post-process stuff for some kinda
//! installed bit.


// XXX These are _very_ similar and we can probably factor out a lot, but
// there's juuust enough small diffs it'd take some doing to make it
// clean, so for the moment, I'm just letting it all repeat.

use std::process::Command;
use std::io::{stdout, Write as _};
use std::path::Path;


/// Do kldxref's for the kernel
pub(crate) fn kldxref(basedir: &Path) -> Result<(), anyhow::Error>
{
	// f-u.sh also does some conditionalization on this, but heck with
	// it, I'm just gonna do it.
	print!("Running kldxref...  ");
	stdout().flush()?;

	const CMD: &str = "/usr/sbin/kldxref";
	let cret = Command::new(CMD)
			.args([
				"-R".as_ref(), basedir.join("boot").as_os_str(),
			]).status()?;
	match cret.success() {
		true  => println!("Done."),
		false => println!("failed\n{cret:?}\n"),
	}

	Ok(())
}


/// Check if sshd is running, and maybe try restarting it.
pub(crate) fn try_sshd_restart() -> Result<(), anyhow::Error>
{
	// We don't even try if you're not root
	if crate::util::euid() != 0 { return Ok(()) }

	// See if we can see it running
	const SVC: &str = "/usr/sbin/service";
	let cret = Command::new(SVC).args(["sshd", "status"]).status()?;
	if !cret .success() { return Ok(()) }

	// It is, kick it.
	println!("Restarting sshd after upgrade.");

	// If it fails, warn very loudly
	let cret = Command::new(SVC).args(["sshd", "restart"]).status()?;
	match cret.success() {
		true  => println!("  Done."),
		false => eprintln!("\nWARNING WARNING WARNING: restart sshd failed\n\
				{cret:?}\n"),
	}

	// And move on
	Ok(())
}


/// Rehash certs
pub(crate) fn rehash_certs(basedir: &Path) -> Result<(), anyhow::Error>
{
	// certctl isn't quiet
	println!("Rehashing certs...  ");
	//stdout().flush()?;

	const CMD: &str = "/usr/sbin/certctl";
	let cret = Command::new(CMD).arg("rehash")
			.env("DESTDIR", basedir).status()?;
	match cret.success() {
		true  => println!("  Done."),
		false => println!("failed\n{cret:?}\n"),
	}

	Ok(())
}


/// Rebuild passwd db
pub(crate) fn pwd_mkdb(basedir: &Path) -> Result<(), anyhow::Error>
{
	print!("Rebuilding passwd db...  ");
	stdout().flush()?;

	const CMD: &str = "/usr/sbin/pwd_mkdb";
	let cret = Command::new(CMD)
			.args([
				"-d".as_ref(), basedir.join("etc").as_os_str(),
				"-p".as_ref(), basedir.join("etc/master.passwd").as_os_str(),
			]).status()?;
	match cret.success() {
		true  => println!("Done."),
		false => println!("failed\n{cret:?}\n"),
	}

	Ok(())
}


/// Rebuild login.conf db
pub(crate) fn cap_mkdb(basedir: &Path) -> Result<(), anyhow::Error>
{
	print!("Rebuilding login.conf db...  ");
	stdout().flush()?;

	const CMD: &str = "/usr/bin/cap_mkdb";
	let cret = Command::new(CMD)
			.args([
				basedir.join("etc/login.conf"),
			]).status()?;
	match cret.success() {
		true  => println!("Done."),
		false => println!("failed\n{cret:?}\n"),
	}

	Ok(())
}


/// Rebuild man indices.
///
/// f-u.sh does some jiggery to see if we need to rebuid anything.  I'm
/// not going to bother; it's fast enough to just let it run.
pub(crate) fn makewhatis(basedir: &Path) -> Result<(), anyhow::Error>
{
	print!("Rebuilding manpage indices...  ");
	stdout().flush()?;

	const CMD: &str = "/usr/bin/makewhatis";

	for mdstr in ["usr/share/man", "usr/share/openssl/man"]
	{
		let mdir = basedir.join(mdstr);
		if !mdir.join("mandoc.db").is_file() { continue; }

		let cret = Command::new(CMD).arg(mdir).status()?;
		match cret.success() {
			true  => { print!("/{mdstr} "); stdout().flush()?; },
			false => { println!("failed on /{mdstr}:\n{cret:?}\n"); },
		}
	}

	println!("Done.");
	Ok(())
}
