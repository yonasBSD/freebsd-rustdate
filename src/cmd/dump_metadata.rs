//! #0 dump-metadata
use std::io::{stdout, Write as _};

use crate::command::CmdArg;



pub(crate) fn run(carg: CmdArg) -> Result<(), anyhow::Error>
{
	// Setup dirs
	let rtdirs = crate::core::RtDirs::init(&carg.config.basedir(),
			&carg.config.workdir())?;

	// Split up and extract our bits
	let CmdArg { clargs, config, version: _ } = carg;

	let dmargs = match clargs.command {
		crate::command::FrCmds::DumpMetadata(ua) => ua,
		_ => unreachable!("I'm a dump_metadata, why does it think I'm not??"),
	};


	// Find server to get info from
	let version = &dmargs.version;
	println!("Loading info for {version}.");
	let mut server = crate::server::Server::find(&config.servername,
			&version, &config.keyprint)?;
	server.set_filesdir(rtdirs.files().to_path_buf());
	let metadatas = &["all", "old", "new"];

	print!("Loading metadata index for {version}...");
	stdout().flush()?;
	let mdidx = server.get_metadata_idx()?;
	println!("   OK.");

	print!("Getting metadata files for {version}...  ");
	stdout().flush()?;
	let metamiss = mdidx.not_in_dir(rtdirs.files(), metadatas);
	match metamiss.len()
	{
		0 => println!("All present."),
		_ => {
			println!("{} missing.", metamiss.len());

			// So grab 'em.
			println!("Fetching...");
			let files = metamiss.into_iter().collect();
			server.fetch_metafiles(files)?;
			println!("Done.");
		},
	};

	print!("Checking metadata file hashes...  ");
	stdout().flush()?;
	let hres = {
		let fd = rtdirs.files();
		let td = rtdirs.tmp();
		mdidx.check_hashes(fd, td, metadatas)
	};
	match hres {
		Ok(_) => println!("   OK."),
		Err(e) => {
			println!("   Errors found.\n{}", e.join("\n"));
			anyhow::bail!("Invalid metafiles, bailing.");
		},
	};


	// And now save out the files.
	let tmpdir = rtdirs.tmp();
	let outdir = &dmargs.dir;
	print!("Writing out metadata files to {}...\n    ", outdir.display());
	stdout().flush()?;
	for md in metadatas
	{
		let infile = mdidx.one_tmpfile(tmpdir, md).unwrap();
		let outfname = format!("fupd-md-index-{md}");
		let outfile = outdir.join(&outfname);

		std::fs::copy(&infile, &outfile)?;
		print!(" {outfname}");
		stdout().flush()?;
	}


	println!("\n\nDone.");
	Ok(())
}
