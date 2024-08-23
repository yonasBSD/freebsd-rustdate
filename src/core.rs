//! Various common core functionality.


/// Runtime dirs (state, temp, etc)
pub(crate) mod rtdirs;
pub(crate) use rtdirs::RtDirs;

/// Generic threadpool implementation
pub(crate) mod pool;

/// FS scanning
pub(crate) mod scan;

/// Hashfile fetching
pub(crate) mod hashfetch;

/// Patching
pub(crate) mod patchcheck;

/// Metadata filtering bits
pub(crate) mod filter;

/// File merging bits
pub(crate) mod merge;

/// Installing bits
pub(crate) mod install;
