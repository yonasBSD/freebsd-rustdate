//! #0 install
use crate::command::CmdArg;
use crate::util::{plural, path_join};
use crate::state::Manifest;
use crate::core::RtDirs;
use crate::core::install;
use crate::metadata::MetadataLine;
use crate::metadata::SplitTypes;

use std::io::{stdout, Write as _};
use std::collections::HashMap;
use std::path::{PathBuf, Path};

use anyhow::bail;


/// Individual command installs could result in several possible actions.
#[derive(Debug)]
enum InstRet
{
	/// Manifest has updated stuff in it, save
	Save,

	/// Install is complete, clear out
	Done,
}



/// Command: $0 install
///
/// Main entry point
pub(crate) fn run(carg: CmdArg) -> Result<(), anyhow::Error>
{
	// Setup dirs
	let rtdirs = RtDirs::init(&carg.config.basedir(),
			&carg.config.workdir())?;

	// Split up
	let CmdArg { clargs, config, version } = carg;

	// Extract our own args
	let args = match clargs.command {
		crate::command::FrCmds::Install(a) => a,
		_ => unreachable!("I'm a install, why does it think I'm not??"),
	};

	// Handle disabling fsync if we asked for that.
	if args.no_sync { install::set_fsync(false); }

	// Load up the state and see what's in the manifest
	let mut state = match rtdirs.state_load_raw()? {
		Some(s) => s,
		None => {
			bail!("No state to load; no fetch/upgrade has been run?");
		},
	};
	let manifest = match &mut state.manifest {
		Some(m) => m,
		None => {
			println!("No install pending.");
			return Ok(());
		},
	};


	// OK, say what we're doing
	let upvers = manifest.version();
	let mt = manifest.mtype();
	let cmdname = crate::util::cmdname();
	println!("Installing pending {mt} from {version} to {upvers}");


	// Do a quick check; if there are conflicted merges, we're not ready
	// to install anyway...
	if let Manifest::Upgrade(mup) = manifest
	{
		use crate::util::plural;
		let ncf = mup.merge_conflict.len();
		if ncf > 0
		{
			println!(" {ncf} merge conflict{} unresolved.", plural(ncf));
			println!("    Run `{cmdname} resolve-merges` to resolve");
			bail!("Unresolved merge conflicts");
		}
	}



	// Rack up some info out of cur/new that we'll use several times.
	let exp_hashes;
	let cn_paths: Vec<_>;
	{
		let (cur, new) = match manifest {
			Manifest::Fetch(f) => {
				(&f.cur, &f.new)
			},
			Manifest::Upgrade(f) => {
				(&f.cur, &f.new)
			},
		};

		exp_hashes = {
			use std::collections::HashSet;
			let mut exp_hashes = HashSet::with_capacity(new.files.len());
			cur.files.values().for_each(|f| { exp_hashes.insert(f.sha256); });
			new.files.values().for_each(|f| { exp_hashes.insert(f.sha256); });
			exp_hashes
		};

		cn_paths = {
			let mut paths = cur.allpaths_hashset();
			new.allpaths_hashset().into_iter().for_each(|p| {
				if !paths.contains(p) { paths.insert(p); }
			});
			paths.into_iter().map(|p| p.to_path_buf()).collect()
		};
	}



	// Check that expected files all exist.
	// f-u.sh install_verify()
	let nhf = exp_hashes.len();
	print!("Checking required files are present...   {nhf} hashfiles...  ");
	stdout().flush()?;
	for h in exp_hashes
	{
		let hb = h.into();
		if !rtdirs.hashfile(&hb).is_file()
		{
			println!("Update files missing -- this should never happen.  \
					Try re-running `{cmdname} {mt}`.");
			bail!("Internal error -- missing files");
		}
	}
	println!("Ok.");



	// Handle boot envs if we should
	'mkbe: {
		use crate::util::bectl;

		// Not doing BE?  Don't BE.
		if !config.create_boot_env { break 'mkbe; }
		// Not installing to root?  Don't BE.
		if config.basedir() != &"/".as_ref() { break 'mkbe; }
		// You're not root?  Don't BE.
		if crate::util::euid() != 0 { break 'mkbe; }
		// BE's not enabled?  Don't BE.
		if !bectl::enabled()? { break 'mkbe; }

		// Dry run?  Don't BE.
		if args.dry_run { break 'mkbe; }

		// OK, we're doing it then.  Make a name pretty much how f-u.sh
		// does.
		let ts = {
			let now = chrono::Local::now();
			let nstr = now.format("%Y-%m-%d_%H%M%S");
			nstr
		};
		let snap = format!("{version}_{ts}");
		print!("Creating snapshot of existing boot environment: ({snap})...  ");
		stdout().flush()?;
		bectl::create(&snap)?;
		println!("Done.");
	}



	// Find any files we might touch that have the schg flag, and unset
	// it from 'em.
	//
	// Strictly, this does too much since e.g. on the multi-step Upgrade
	// side, we'd only _really_ want to unschg the files we're going to
	// deal with on this step, but, well, f-u.sh doesn't try that hard,
	// so neither will we.
	//
	// f-u.sh install_unschg()
	let cnlen = cn_paths.len();
	println!("Checking file flags ({cnlen} path{} to scan)", plural(cnlen));
	let schgs = {
		use crate::core::scan;
		let bd = config.basedir().to_path_buf();
		scan::schg(bd, cn_paths)?
	};
	let nschg = schgs.len();
	if nschg == 0
	{
		println!("No +schg files found.");
	}
	else
	{
		let nr_msg = || format!("{nschg} +schg file{} found, but you're \
				not root, so you can't clear them.", plural(nschg));

		if args.dry_run
		{
			match crate::util::euid()
			{
				0 => println!("{nschg} +schg file{} found, clearing   (dry run)",
						plural(nschg)),
				_ => println!("{}  (dry run)", nr_msg()),
			};
		}
		else
		{
			if crate::util::euid() != 0
			{
				// You're not root, you can't chflags...
				anyhow::bail!(nr_msg());
			}

			print!("{nschg} +schg file{} found.  Clearing flags...",
					plural(nschg));
			stdout().flush()?;
			for f in schgs
			{
				use crate::util::unschg_file;
				let fpath = path_join(config.basedir(), &f.0);
				unschg_file(&fpath, f.1)?;
			}
			println!("Done.");
		}
	}



	/*
	 * OK, now we can start looking at the individual steps we take.
	 * f-u.sh does a fair bit of twisty stuff to support fetch/upgrade
	 * together, since it's lost the distinction by this point, aside
	 * from some weird flags it touch's.  We don't, so I'll just write
	 * them up separately.
	 *
	 * For fetch's, we just assume the system all works with any
	 * cross-versioning, so we just blat everything into place.  That's
	 * easy.  Not at all scary or anything.
	 *
	 * For upgrade's, we 3-step it; install the kernel, wait for them to
	 * reboot, then install the world, wait for them to deal with
	 * rebuilding anything, then remove the old .so.*'s.
	 */
	println!("Beginning install.\n");
	let iret = match manifest {
		Manifest::Fetch(_)   => fetch(&args, &rtdirs, &config, manifest)?,
		Manifest::Upgrade(_) => upgrade(&args, &rtdirs, &config, manifest)?,
	};


	// XXX f-u.sh has rollback, I'm not doing that right now...


	// Depending on the result, do the appropriate thing before
	// returning.  If it's a dry run, the appropriate thing is always
	// nothing, so...
	if !args.dry_run
	{
		match iret
		{
			InstRet::Save => {
				// Updated manifest, save it
				rtdirs.state_save(&state)?;
			},
			InstRet::Done => {
				// Install done, clear it
				state.manifest = None;
				rtdirs.state_save(&state)?;
			},
			// None -> doesn't exist anymore...  if the subfuncs finish
			// cleanly, it's because they either did something (so we're
			// doing something) or it was a dry run (and we already
			// skipped this).
		}
	}


	Ok(())
}




/*
 * Fetch/Upgrade individual variants
 */
use crate::command::FrCmdInstall;
use crate::config::Config;

/// Do the install for a 'fetch' invocation
fn fetch(args: &FrCmdInstall, rtdirs: &RtDirs, config: &Config,
		manifest: &Manifest)
		-> Result<InstRet, anyhow::Error>
{
	let dry = args.dry_run;

	// We're going a bit roundabout and with some unnecessary indirection
	// and allocation, to use methods we already have and not try to be
	// oversmart, so this could be made more efficient along multiple
	// axes.  But we're also gzip'ing and hitting the filesystem on the
	// one hand, and competing with a shell script on the other, so heck
	// with that.  Let malloc and memcpy earn their paychecks.

	// While we still have the assembled manifest, pull out the change
	// summary and spit it out.
	let csum = manifest.change_summary();
	use crate::state::ManifestSummary;
	let ManifestSummary { added, removed, updated} = csum;

	// Now split down; we know it's a fetch
	let mf = match manifest {
		Manifest::Fetch(f) => f,
		_ => unreachable!("No, this has to be a ManiFetch..."),
	};

	// Now we can put together what to do here.  We're going to need to
	// install the things in either added or updated, and then later
	// delete the things in removed.  So let's go through the
	// added/updated and get the MetadataLine's for 'em.
	use crate::util::uniq_vecs;
	let ipaths = uniq_vecs(&mut [added, updated]);
	let ilines = mf.new.get_from_paths(ipaths);

	// Split out into the different types.
	let smd = split_metadata(ilines);


	//
	// And now the action
	//

	// Do the kernel backup first.
	if !dry { install::backup_kernel(config.basedir())?; }

	// Install the bits
	install::split(smd, rtdirs, config.basedir(), dry)?;

	// Delete things that need deleting
	match handle_removes(&removed, config.basedir(), dry)?
	{
		None => (),
		Some(fail) => rmdirs_fails_warn(&fail),
	}


	// kldxref on non-dry
	if !dry { install::kldxref(config.basedir())?; }

	// Kick the postworld bits
	if !dry { post_world(config.basedir())? };


	// And that's it.  Fetch is a single step, so if we make it this far,
	// the install is all done (if we did stuff, anyway).
	println!("\n\nInstall complete{}.",
			if dry { "   (dry run)" } else { "" });
	Ok(InstRet::Done)
}



/// Do the install for a 'upgrade' invocation
fn upgrade(args: &FrCmdInstall, rtdirs: &RtDirs, config: &Config,
		manifest: &mut Manifest)
		-> Result<InstRet, anyhow::Error>
{
	// Dry run upgrade is a little trickier, since we have to run all 3
	// steps.
	let dry = args.dry_run;

	let cmdname = crate::util::cmdname();


	// Summaryize the summary
	let csum = manifest.change_summary();
	use crate::state::ManifestSummary;
	let ManifestSummary { added, removed, updated} = csum;

	// Pull out the ManiUpgrade
	let mu = match manifest {
		Manifest::Upgrade(u) => u,
		_ => unreachable!("No, this has to be a ManiUpgrade..."),
	};


	// No matter what step we're doing, we'll build up that to-do list,
	// then filter it down as necessary.
	use crate::util::uniq_vecs;
	let ipaths = uniq_vecs(&mut [added, updated]);
	let ilines = mu.get_from_paths(ipaths);
	// Don't split yet, 'till we work out what step we're doing.



	/*
	 * OK, now, what are our steps?  Upgrades do a 3-step install
	 * process, with the user presumably doing appropriate things in
	 * between.
	 *
	 * - Install kernel
	 *   (user reboots)
	 * - Install everything else
	 *   (user rebuilds things, reinstalls pkgs, etc)
	 * - Delete old libs
	 *   (user freaks out over the things they forgot to rebuild)
	 */
	use crate::util::is_kernel_dir;


	// Do we need to install the kernel stuff?
	if !mu.kernel
	{
		// Kernel means "everything that starts with /boot" by our
		// meaning, so strip down to those things.
		println!("Installing kernel...");

		// Backup the kernel first
		if !dry { install::backup_kernel(config.basedir())?; }

		// Filter down our install/remove lists.
		let klines: HashMap<_, _> = ilines.iter().filter_map(|(p, m)| {
			match is_kernel_dir(p) {
				false => None,
				true  => Some((p.clone(), m.clone())),
			}
		}).collect();
		let kremoved: Vec<_> = removed.iter().filter_map(|p| {
			match is_kernel_dir(p) {
				false => None,
				true  => Some(p),
			}
		}).collect();

		// Now we can smd-ify
		let smd = split_metadata(klines);

		// Do the install/delete
		install::split(smd, rtdirs, config.basedir(), dry)?;
		match handle_removes(&kremoved, config.basedir(), dry)?
		{
			None => (),
			Some(fail) => rmdirs_fails_warn(&fail),
		}

		// kldxref on non-dry
		if !dry { install::kldxref(config.basedir())?; }

		// If this wasn't a dry run, and we got here, we're done.  Dry
		// runs would quietly proceed ahead.
		mu.kernel = true;
		if dry
		{
			println!("\nKernel updated installed.  (dry run, continuing)\n");
		}
		else
		{
			println!("\nKernel updates have been installed.  Please reboot \
					and run\n`{cmdname} install` again to finish \
					installing updates.");
			match args.all
			{
				true => println!("\n  (run with --all, proceeding anyway)\n"),
				false => return Ok(InstRet::Save),
			}
		}
	}


	// Need to install the world?
	if !mu.world
	{
		println!("Installing world...");

		// Well, first of all, world doesn't include the stuff we did in
		// the kernel dir above.
		let wlines: HashMap<_, _> = ilines.iter().filter_map(|(p, m)| {
			match is_kernel_dir(p) {
				true  => None,
				false => Some((p.clone(), m.clone())),
			}
		}).collect();
		let mut wremoved: Vec<_> = removed.iter().filter_map(|p| {
			match is_kernel_dir(p) {
				true  => None,
				false => Some(p),
			}
		}).collect();

		// World does a lot of piecewise stuff, to try and keep things
		// safe on the way through.  f-u.sh's process goes like:
		//
		// - Create dirs
		// - Install ld-elf
		// - Install .so's
		// - Install everything else
		// - Delete only the stuff that falls into the prior step; that
		//   is, old dirs, ld-elf, and .so's don't get anything done with
		//   them here.
		//
		// I'm going to be doing some minor variations on that.
		// do_mdl_installs() already handles ordering and batching like
		// that within a type, and I've punted on cross-type ordering
		// like that.  So for our purposes here, that boils down to:
		//
		// - Do the install of what I worked out above
		// - Filter down the removals to cut out ld-elf and .so's, but go
		//   ahead and [try to] delete the dirs.

		// So we just install what we worked out, like usual.
		let smd = split_metadata(wlines);
		install::split(smd, rtdirs, config.basedir(), dry)?;

		// And remove everything that doesn't match ld/.so.  Make a list
		// of the .so's we'd remove for a message...
		let linker = install::re_linker_file();
		let shlib = install::re_so_file();
		let mut rm_sos = false;
		wremoved.retain(|p| {
			let pstr = String::from_utf8_lossy(p.as_os_str().as_encoded_bytes());
			if linker.is_match(&pstr) { return false; }
			if shlib.is_match(&pstr)  { rm_sos = true; return false; }
			true
		});
		handle_removes(&wremoved, config.basedir(), dry)?;
		// Don't bother warning here.


		// Now do the postworld stuff
		if !dry { post_world(config.basedir())?; }


		// OK, world done.  If there are so's to remove, stop here and
		// give the user a change to screw up.
		mu.world = true;
		if dry
		{
			println!("\nWorld updated installed.  (dry run, continuing)\n");
		}
		else
		{
			println!("\nWorld update installed.");
			if rm_sos
			{
				println!("\
					Completing this upgrade requires removing old shared \
					object files.\n\
					Please rebuild all installed 3rd party software \
					(e.g., programs installed from the ports tree) and then \
					run\n`{cmdname} install`  again to finish installing \
					updates.");
				match args.all
				{
					true => println!("\n  (run with --all, proceeding anyway)\n"),
					false => return Ok(InstRet::Save),
				}
			}
		}
	}


	// Now do final cleanup.  f-u.sh does some grepping around to try and
	// find the things that weren't already cleaned up in the earlier
	// steps, but screw that, I'll just redo _all_ the deletes.
	match handle_removes(&removed, config.basedir(), dry)?
	{
		None => (),
		Some(fail) => rmdirs_fails_warn(&fail),
	}



	// I guess we're done.
	println!("\n\nUpgrade complete{}.",
			if dry { "   (dry run)" } else { "" });
	Ok(InstRet::Done)
}




/*
 * Internal shared funcs
 *
 * XXX This is getting bit enough I should really be splitting it out of
 * this file...
 */

/// Given a set of paths and MetadataLine's, split them out into the
/// individual install types.
fn split_metadata(mds: HashMap<PathBuf, MetadataLine>) -> SplitTypes
{
	SplitTypes::from_map_lines(mds)
}



/// Handle removing files
fn handle_removes(rms: &[impl AsRef<Path>], basedir: &Path, dry: bool)
		-> Result<Option<Vec<PathBuf>>, anyhow::Error>
{
	let rmlen = rms.len();
	if rmlen == 0 { return Ok(None); }

	if dry
	{
		println!("{rmlen} file{} to remove   (dry run)", plural(rmlen));
		return Ok(None);
	}

	// Else there's stuff to do.
	let mut rdirs = Vec::new();
	print!("Deleting {rmlen} path{}...   ", plural(rmlen));
	stdout().flush()?;

	// In practice, rms is probably already always sorted, but be safe
	// and sort ourselves.  And then reverse it; we want to rm
	// reverse-depth-first, otherwise we find that a dir is not empty and
	// fail to delete it, before we delete the stuff in it...
	use itertools::Itertools as _;
	for p in rms.iter().map(|p| p.as_ref()).sorted_unstable().rev()
	{
		let rmp = path_join(basedir, p);
		if install::rm(&rmp)? { rdirs.push(p); }
	}
	println!("Done.");


	match rdirs.len() {
		0 => Ok(None),
		_ => Ok(Some(rdirs.into_iter().map(|p| p.to_path_buf()).collect())),
	}
}

/// Use the return from handle_removes() to warn.  Can make this smarter
/// maybe...
fn rmdirs_fails_warn(dirs: &[impl AsRef<Path>])
{
	let dlen = dirs.len();
	if dlen > 0
	{
		println!("{dlen} director{} not removed:",
			if dlen > 1 { "ies" } else { "y" });
		for d in dirs { println!("  {}", d.as_ref().display()); }
	}
}



/// Post-world-install rebuilding stuff.
fn post_world(basedir: &Path) -> Result<(), anyhow::Error>
{
	let atroot = basedir == &"/".as_ref();

	// Restart sshd if it's running, since some cases where bits of it
	// are updated behind its back could cause future logins to fail.
	// (PR263489).  Only if we're working on the root system, of
	// course...
	if atroot { install::try_sshd_restart()?; }

	// Rehash SSL certs.  certctl(1) has been around since 12.2; f-u.sh
	// tries to support systems before that.  I don't.
	install::rehash_certs(basedir)?;

	// Rebuild passwd and login class DB
	install::pwd_mkdb(basedir)?;
	install::cap_mkdb(basedir)?;

	// And unconditionally eat the work of rebuilding man indices
	install::makewhatis(basedir)?;


	// Guess that's it...
	Ok(())
}
