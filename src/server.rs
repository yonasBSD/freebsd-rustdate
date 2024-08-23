//! Various server-related functionality.  Most interaction with the
//! freebsd-update server will route through here somehow.


/// Base defs
mod server;
pub(crate) use server::Server;

/// Looking up and building server info (SRV lookups, etc)
pub(crate) mod lookup;

/// General http bits
mod http;

/// Bit for loading public key and "tag" (basic metadata) from a server
mod keytag;

/// Loading metadata stuff from the server
mod metadata;
