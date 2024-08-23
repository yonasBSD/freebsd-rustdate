//! Bits related to loading key/tag from a server
use super::Server;
use crate::info::version::AVersion;



/// Data about a key and the tag info
#[derive(Debug, Default)]
pub(in crate::server) struct KeyTag
{
	// /// The public key from the server: should match the hash of it we
	// /// have in the config, and is used to decrypt the below fields from
	// /// the 'tag' file.
	// pub(in crate::server) key: String,

	/// The patchlevel the server claims to have
	pub(in crate::server) patch: Option<u32>,

	/// The hash of the metadata index file
	pub(in crate::server) tidx: String,

	/// The EOL of the offered release
	pub(in crate::server) eoltime: i64,
}


// Parse out a string of the keytag into our struct
impl KeyTag
{
	fn from_str(s: &str, xarch: &str, xvers: &AVersion)
			-> Result<Self, anyhow::Error>
	{
		use anyhow::{anyhow, bail};

		let patch: Option<u32>;
		let tidx: String;
		let eoltime: i64;

		let mut tbits = s.split(|c| c == '|');
		match tbits.next() {
			Some("freebsd-update") => (),
			x => bail!("Expected freebsd-update, got {x:?}"),
		};
		match tbits.next() {
			Some(s) if s == xarch => (),
			x => bail!("Expected arch {xarch}, got {x:?}"),
		};
		match tbits.next() {
			Some(s) => {
				// This is a little silly, but saves building another String
				if !s.starts_with(&xvers.release)
				{ bail!("Expected release {}, got {s}", xvers.release); }
				if !s.ends_with(&xvers.reltype)
				{ bail!("Expected reltype {}, got {s}", xvers.reltype); }
				()
			},
			x => bail!("Expected release info, got {x:?}"),
		};
		match tbits.next() {
			Some(s) => {
				let p: u32 = s.parse()
						.map_err(|e| anyhow!("Can't parse patch: {}", e))?;
				patch = match p {
					0 => None,
					p => Some(p),
				};
			},
			x => bail!("Expected patch, got {:?}", x),
		};
		match tbits.next() {
			Some(s) => tidx = s.to_string(),
			x => bail!("Expected tindex hash, got {:?}", x),
		};
		match tbits.next() {
			Some(s) => {
				// Has a trailing \n that causes hiccups
				eoltime = s.trim().parse()
						.map_err(|e| anyhow!("Can't parse eoltime: {}", e))?
			},
			x => bail!("Expected eol timestamp, got {:?}", x),
		};


		// Well, I guess we got it all, huh?
		let kt = KeyTag {
			// key,
			patch,
			tidx,
			eoltime,
		};
		Ok(kt)
	}
}


impl Server
{
	/// Try fetching the key and "tag" from a Server.  Returns the info
	/// if we got it, or some sorta error if we didn't.
	pub(crate) fn get_key_tag(&mut self, vers: &AVersion, keyprint: &str)
			-> Result<(), anyhow::Error>
	{
		// Gets used in a few places...
		let arch = crate::info::kernel::arch()?;

		// Build up what our URL's will look like.  We probably won't
		// have this yet...
		let burl = match &mut self.cache.burl {
			Some(u) => u,
			None => {
				let burl = format!("http://{}/{}-{}/{}/", self.host,
						vers.release, vers.reltype, arch);
				let burl = url::Url::parse(&burl)?;
				self.cache.burl = Some(burl);
				self.cache.burl.as_ref().unwrap()
			},
		};

		let kurl = burl.join("pub.ssl")?;
		let turl = burl.join("latest.ssl")?;


		// Setup HTTP requesting and loading
		let agent = crate::server::http::mk_agent();


		// Load in the key: just a blob of bytes.  But we should know its
		// hash...
		// X-ref comment on get_bytes() about how it's used differently
		// here than anywhere else.
		let key = super::http::get_bytes(&agent, &kurl)?;
		use crate::util::hash;
		hash::check_sha256(&key, &keyprint, "public key")?;
		// And it's a PEM-encoded thing, so we can just call it a String.
		let key = String::from_utf8(key)?;

		// OK, now load up the tag; we'll do more processing
		let tag = super::http::get_bytes(&agent, &turl)?;

		// Wacky handrolled crypto, what fun
		let tag = decrypt_tag(key.as_bytes(), &tag)?;

		// Parse it out of the string
		let kt = KeyTag::from_str(&tag, &arch, vers)?;

		// Stash that and the agent
		self.cache.agent = Some(agent);
		self.cache.keytag = Some(kt);

		Ok(())
	}
}




/*
 * The rest of this is just internal implementation details of the
 * external entries above.
 */




/// Deal with wacky handrolled crypto.  How often do you get to see
/// people encrypting payloads with RSA?
#[derive(Debug)]
#[derive(thiserror::Error)]
enum DecryptError
{
	/// OpenSSL decryption error
	#[error("OpenSSL error: {0}")]
	OpenSSL(#[from] openssl::error::ErrorStack),

	/// UTF8 parsing error.  Technically I s'pose this doesn't _have_ to
	/// be the case, but life is just easier if we use String's where we
	/// can.
	#[error("Bad UTF8 found: {0}")]
	Utf8(#[from] std::string::FromUtf8Error),
}

fn decrypt_tag(key: &[u8], data: &[u8]) -> Result<String, DecryptError>
{
	// Put together the pubkey
	use openssl::rsa::{Rsa, Padding};
	let pub_rsa = Rsa::public_key_from_pem(&key)?;

	// Decrypt
	let mut dedata: Vec<u8> = vec![0; pub_rsa.size() as usize];
	pub_rsa.public_decrypt(&data, &mut dedata, Padding::PKCS1)?;

	// String-ify and hand back.  Gotta clear padding NULL's manually?
	let mut destr = String::from_utf8(dedata)?;
	destr.retain(|c| c != '\0');
	Ok(destr)
}



#[cfg(test)]
pub(super) mod tests
{
	use super::*;

	#[test]
	fn crazy_crypto()
	{
		// Test out the decrypt with whatever I just grabbed from the
		// server.
		let pubkey = include_bytes!("test_data/pub.ssl");
		let tagenc = include_bytes!("test_data/latest.ssl");
		let tag_expect = include_bytes!("test_data/latest");

		let tagdec = decrypt_tag(pubkey, tagenc).unwrap();
		assert_eq!(tagdec.as_bytes(), tag_expect);
	}

	#[test]
	fn parse_keytag_simple()
	{
		let ktstr = "freebsd-update|amd64|1.2-RELEASE|2|hashashashash|12345";
		let xarch = "amd64";
		let xversion: AVersion = "1.2-RELEASE".parse().unwrap();
		let kt = KeyTag::from_str(ktstr, xarch, &xversion).unwrap();
		assert_eq!(kt.patch, Some(2), "Should be -p2");
		assert_eq!(kt.tidx, "hashashashash", "Got the 'right' hash");
		assert_eq!(kt.eoltime, 12345, "Got the 'right' eoltime");
	}

	#[test]
	fn parse_keytag_patch0()
	{
		let ktstr = "freebsd-update|amd64|1.2-RELEASE|0|hashashashash|12345";
		let xarch = "amd64";
		let xversion: AVersion = "1.2-RELEASE".parse().unwrap();
		let kt = KeyTag::from_str(ktstr, xarch, &xversion).unwrap();
		assert_eq!(kt.patch, None, "-p0 should be None");
	}

	#[test]
	fn parse_keytag_bag()
	{
		let ktstr = "freebsd-update|amd64|1.2-RELEASE|0|hashashashash|12345";

		// Wrong arch
		let xarch = "i386";
		let xversion: AVersion = "1.2-RELEASE".parse().unwrap();
		let kte = KeyTag::from_str(ktstr, xarch, &xversion).unwrap_err();
		let estr = kte.to_string();
		assert!(estr.contains("Expected arch i386, got"), "Got arch err");

		// Wrong release
		let xarch = "amd64";
		let xversion: AVersion = "2.3-RELEASE".parse().unwrap();
		let kte = KeyTag::from_str(ktstr, xarch, &xversion).unwrap_err();
		let estr = kte.to_string();
		let xerr = "Expected release 2.3, got 1.2-RELEASE";
		assert_eq!(estr, xerr, "Got release err");

		// Wrong reltype
		let xversion: AVersion = "1.2-BETA".parse().unwrap();
		let kte = KeyTag::from_str(ktstr, xarch, &xversion).unwrap_err();
		let estr = kte.to_string();
		let xerr = "Expected reltype BETA, got 1.2-RELEASE";
		assert_eq!(estr, xerr, "Got reltype err");
	}
}
