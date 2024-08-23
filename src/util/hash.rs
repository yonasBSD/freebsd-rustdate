//! Misc (SHA256) hashing utils
use std::ops::Deref;
use std::fmt;
use serde_with::{serde_as, hex::Hex};


/// A raw SHA256 hash output.
///
/// SHA256 gives you a 256 bit number, which you need 256 bits to store.
/// Or more, depending on how you store it, but if you wanna be simple,
/// it's just 256 bits.  Or 32 octets.  The sha256 crate stores into a
/// [u8; 32], and the base16 crate can deal with that, so we just wrap
/// that and call it good.
#[derive(Default, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[serde_as]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Sha256Hash(
	#[serde_as(as = "Hex")]
	[u8; 32]
);

impl Deref for Sha256Hash
{
	type Target = [u8; 32];
	fn deref(&self) -> &Self::Target { &self.0 }
}

impl fmt::Debug for Sha256Hash
{
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
	{ write!(f, "Sha256Hash({})", self.to_buf().as_ref()) }
}

impl std::str::FromStr for Sha256Hash
{
	type Err = anyhow::Error;

	fn from_str(s: &str) -> Result<Self, Self::Err>
	{
		use anyhow::anyhow;

		// First check the length; that's easy
		let xlen = 64;
		let slen = s.len();
		if slen != xlen
		{
			let estr = anyhow!("Invalid hash length: {slen} should be \
					{xlen} for '{s}'");
			Err(estr)?;
		}

		// And dehexify
		let mut hout = Sha256Hash::default();
		let hret = base16ct::lower::decode(&s, &mut hout.0)
				.map_err(|e| anyhow!("Invalid hex parsing: {e} trying '{s}'"))?;

		// Double-checking the len here, in case I made a booboo.
		assert_eq!(hret.len(), hout.len(), "should have gotten the hex len right");

		Ok(hout)
	}
}

impl From<[u8; 32]> for Sha256Hash
{
	fn from(buf: [u8; 32]) -> Self
	{
		Self(buf)
	}
}

impl From<&Sha256HashBuf> for Sha256Hash
{
	fn from(buf: &Sha256HashBuf) -> Self
	{
		buf.as_ref().parse().unwrap()
	}
}

impl fmt::Display for Sha256Hash
{
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
	{
		let hb: Sha256HashBuf = self.to_buf();
		write!(f, "{}", hb)
	}
}

impl Sha256Hash
{
	pub(crate) fn to_buf(&self) -> Sha256HashBuf { self.clone().into() }
}



/// A hex SHA256 output.
///
/// A base16 encoding of a number is inherently valid UTF-8, so trivially
/// String-able too.  But since we know the size, we go with a more
/// fixed-size allocation type for simplicity, when we don't need a
/// str-ified version.
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[serde_as]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Sha256HashBuf(
	#[serde_as(as = "Hex")]
	[u8; 64]
);

impl Deref for Sha256HashBuf
{
	type Target = [u8; 64];
	fn deref(&self) -> &Self::Target { &self.0 }
}

impl Default for Sha256HashBuf
{
	// Can't just derive for 64-size arrays, until the Glorious Future of
	// some combination of const generics and specialization and
	// whatnot...
	fn default() -> Self { Self ( [0; 64] ) }
}

impl From<Sha256Hash> for Sha256HashBuf
{
	fn from(hash: Sha256Hash) -> Self
	{
		let mut buf = Self::default();
		let bret = base16ct::lower::encode(&hash.0, &mut buf.0)
				.map_err(|e| format!("Hash encoding error: {}", e))
				.unwrap();

		// Double check that somebody doesn't screw up the length.
		let slen = bret.len();
		let blen = buf.len();
		if slen != blen
		{
			panic!("Programmer screwed up buffer size: should have \
					{blen} but got {slen} encoded");
		}

		// Extra double check; a hex string _must_ be valid UTF-8.  This
		// is really probably a waste of time, but...
		std::str::from_utf8(&buf.0).expect("base16 encode screwed us");

		// 'zit
		buf
	}
}

impl AsRef<str> for Sha256HashBuf
{
	/// Should be impossible to create these other than via our
	/// constructors, so should be guaranteed already UTF-8-y.
	fn as_ref(&self) -> &str
	{
		std::str::from_utf8(&self.0).expect("base16 encode screwed us")
	}
}

impl fmt::Display for Sha256HashBuf
{
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
	{ write!(f, "{}", self.as_ref()) }
}

impl fmt::Debug for Sha256HashBuf
{
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
	{ write!(f, "Sha256HashBuf({})", self.as_ref()) }
}



/*
 * Now some of the hashing utils, using those structs
 */




/// Check the SHA256 hash of a buffer against an expected result.
pub(crate) fn check_sha256(buf: &[u8], expect: &str, name: &str)
		-> Result<(), anyhow::Error>
{
	use sha2::{Sha256, Digest};

	// What we expect
	let xhash: Sha256Hash = expect.parse()?;

	// What we got
	let khash = Sha256::digest(&buf);
	let khash = Sha256Hash(khash.into());

	// Is they ain't?
	if khash != xhash
	{
		use anyhow::anyhow;
		let es = anyhow!("Bad {name} hash: expected '{xhash}', got '{khash}'");
		return Err(es);
	}

	Ok(())
}


#[derive(Debug)]
#[derive(thiserror::Error)]
pub(crate) enum Sha256ReaderErr
{
	#[error("I/O error: {0}")]
	IO(#[from] std::io::Error),

	#[error("Invalid hash (expected {0}, got {1})")]
	Hash(String, String),

	#[error("Invalid expected hash: {0}")]
	Expected(anyhow::Error),
}


/// Calculate the SHA256 of something we can read from (like a filehandle, or
/// a stream out of gzip -d or something).
pub(crate) fn sha256_reader<T: std::io::Read>(rdr: &mut T)
		-> Result<Sha256Hash, Sha256ReaderErr>
{
	use sha2::{Sha256, Digest};

	let mut hasher = Sha256::new();
	std::io::copy(rdr, &mut hasher)?;
	let khash = hasher.finalize();
	let khash = Sha256Hash(khash.into());
	Ok(khash)
}


/// Calculate the SHA256 of a file
pub(crate) fn sha256_file(file: &std::path::Path)
		-> Result<Sha256Hash, Sha256ReaderErr>
{
	let mut fh = std::fs::File::open(file)?;
	sha256_reader(&mut fh)
}


/// Check the SHA256 of something we can read from (like a filehandle, or
/// a stream out of gzip -d or something) against an expected value.
pub(crate) fn check_sha256_reader<T: std::io::Read>(rdr: &mut T, expect: &str)
		-> Result<(), Sha256ReaderErr>
{
	use Sha256ReaderErr as ERR;

	let xhash: Sha256Hash = expect.parse()
			.map_err(|e| ERR::Expected(e))?;
	let gothash = sha256_reader(rdr)?;

	if xhash != gothash
	{
		return Err(ERR::Hash(xhash.to_string(), gothash.to_string()));
	}
	Ok(())
}


/// Check the SHA256 of a file against an expected value.
pub(crate) fn check_sha256_file(file: &std::path::Path, expect: &str)
		-> Result<(), Sha256ReaderErr>
{
	let mut fh = std::fs::File::open(file)?;
	check_sha256_reader(&mut fh, expect)
}



#[cfg(test)]
mod tests
{
	fn start_at_the_beginning() -> &'static str
	{ "Do, a deer, a female deer" }
	fn expect_at_the_beginning() -> &'static str
	{ "762e31fc5d92b2c6d7e5a9485cab35714f5e27457e252d0126663554280099fe" }

	#[test]
	fn sha256()
	{
		let buf = start_at_the_beginning().as_bytes();
		let expect = expect_at_the_beginning();
		super::check_sha256(buf, &expect, "Julie Andrews").unwrap();
	}

	#[test]
	fn sha256_reader()
	{
		let mut buf = start_at_the_beginning().as_bytes();
		let expect = expect_at_the_beginning();
		super::check_sha256_reader(&mut buf, &expect).unwrap();
	}
}
