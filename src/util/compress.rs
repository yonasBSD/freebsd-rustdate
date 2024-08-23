//! Compression-related utils
use std::path::{Path, PathBuf};

use std::io::Write;


/// Decompress a .gz file into a `Writer`er.
pub(crate) fn decompress_gz_write(src: &Path, dst: &mut impl Write)
		-> Result<(), anyhow::Error>
{
	use std::fs::File;

	// Let anyhow tell us a little
	use anyhow::Context as _;
	let ctx = || format!("decompressing from {}", src.display());

	// Hook the input file up to the unzipper
	let gzfh = File::open(src).with_context(|| ctx())?;
	let mut gzd = flate2::read::GzDecoder::new(gzfh);

	// Copy bits, sync it all, and return
	std::io::copy(&mut gzd, dst)?;
	Ok(())
}

/// Decompress a .gz file into a named output location.
pub(crate) fn decompress_gz_file(src: &Path, dst: &Path)
		-> Result<(), anyhow::Error>
{
	use std::fs::File;

	// Open up the output
	let outfh = File::create(dst)?;

	// Wrap up some buffering, and copy
	{
		use std::io::BufWriter;
		use crate::util::FILE_BUFSZ;
		let mut bw = BufWriter::with_capacity(FILE_BUFSZ, outfh);
		decompress_gz_write(src, &mut bw)?;
		bw.flush()?;
	}

	// There we go
	Ok(())
}

/// Decompress a named gz file.  This expects a file of "something.gz",
/// and extracts $srcdir/something.gz to $dstdir/something.
///
/// XXX Not clear this is a useful abstraction...
pub(crate) fn decompress_gz_dirs<T: AsRef<str>>(srcdir: &Path,
		dstdir: &Path, file: T)
		-> Result<PathBuf, anyhow::Error>
{
	// What's the output file?
	let bname = file.as_ref().trim_end_matches(".gz");
	let outf = dstdir.join(bname);

	// Shortcut; if it exists, just return
	if outf.is_file() { return Ok(outf); }

	// Do the decompression
	let gzf = srcdir.join(file.as_ref());
	decompress_gz_file(&gzf, &outf)?;

	// And say where we put it
	Ok(outf)
}


/// Decompress into memory.  Usually you'd want to use decompress_gz()
/// into a file  to do manipulation, but sometimes we only decompress to
/// do something with the bytes in memory, so let the filesystem take a
/// smoke break...
pub(crate) fn decompress_to_vec(src: &Path) -> Result<Vec<u8>, anyhow::Error>
{
	use std::fs::metadata;
	// Open up the input

	// Guess at a decompressed size.  Assume (based on a rigorous random
	// examination and the use of nice round numbers) gzip is saving us
	// about a third on average, so come up with a reasonable starting
	// size.  Might save us a couple mallocs anyway...
	let dsz = match metadata(src) {
		Ok(m)  => m.len() + (m.len() / 2),
		Err(_) => 8192,
	};
	let dsz = std::cmp::min(dsz, 4096).try_into().unwrap();
	let mut out = Vec::with_capacity(dsz);

	// And decompress the data off into it
	decompress_gz_write(src, &mut out)?;
	Ok(out)
}


/// Compress a file to gz.
pub(crate) fn compress_gz(src: &Path, dst: &Path) -> Result<(), std::io::Error>
{
	use std::io::{copy, BufReader};
	use std::fs::File;
	use flate2::write::GzEncoder;
	use flate2::Compression;

	// Open up in/out files and hook up the compressor
	let srcf = File::open(src)?;
	let mut srcr = BufReader::new(srcf);
	let dstf = File::create(dst)?;
	let mut gzc = GzEncoder::new(dstf, Compression::default());

	// Cram it through, sync and return
	copy(&mut srcr, &mut gzc)?;
	let dstf = gzc.finish()?;
	dstf.sync_all()?;
	Ok(())
}
