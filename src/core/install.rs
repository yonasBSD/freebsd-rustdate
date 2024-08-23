//! Various install pieces.


/// Kernel backup bits
mod kernel;
pub(crate) use kernel::backup_kernel;

/// Installing individual bits (files, dirs, etc)
mod bits;
pub(crate) use bits::{dir, file, link, symlink, flags, rm};

/// Rolled up installing routines
mod install;
pub(crate) use install::split;
pub(crate) use install::{re_linker_file, re_so_file};

/// Post-install bits
mod post;
pub(crate) use post::{kldxref, try_sshd_restart, rehash_certs, pwd_mkdb};
pub(crate) use post::{cap_mkdb, makewhatis};


/// fsync() files?
///
/// XXX Make a command-line flag for this...
use std::sync::atomic::{self, AtomicBool};
static FSYNC: AtomicBool = AtomicBool::new(true);

/// Override the default fsyncing
pub(crate) fn set_fsync(s: bool) { FSYNC.store(s, atomic::Ordering::Relaxed) }

/// Should installed files get fsync()'d before moving ?
fn fsync() -> bool { FSYNC.load(atomic::Ordering::Relaxed) }
