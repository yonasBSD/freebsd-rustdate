//! Bits related to loading metadata from the server

/// Metdata file handline
mod files;



use crate::metadata::MetadataIdx;



impl super::Server
{
	/// Load up the metadata index
	pub(crate) fn get_metadata_idx(&mut self)
			-> Result<MetadataIdx, anyhow::Error>
	{
		// This will have already been populated unless the programmer
		// screwed up.
		let burl = self.cache.burl()?;
		let tag  = self.cache.keytag()?;

		// Build URL down to the tag.  Double join seems... odd, surely
		// there's a better way?  Ideally without indirecting through
		// allocating a new String etc?
		let burl = burl.join("t/")?;
		let burl = burl.join(&tag.tidx)?;

		// Load it down
		let idxbytes = self.get_bytes(&burl)?;

		// Check it; should match the hash we fetched it with
		use crate::util::hash;
		hash::check_sha256(&idxbytes, &tag.tidx, "metadata index")?;

		// Now parse it out
		let idx = MetadataIdx::parse(&idxbytes)?;

		// Return
		Ok(idx)
	}


	/// Fetch down metadata files
	pub(crate) fn fetch_metafiles(&self, files: Vec<String>)
			-> Result<u32, anyhow::Error>
	{
		// We just need to know the src URL and the dst dir, the rest is
		// common.
		let mburl = self.cache.burl()?.join("m/")?;
		let fdir  = self.cache.filesdir()?.to_path_buf();

		// And let the lower level do the work
		self.fetch_files_from_to(mburl, files, fdir)
	}
}
