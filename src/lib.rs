//! Main freebsd-rustdate impl lib

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Config
pub mod config;

// Commands and args
pub mod command;

// Components-related bits
pub mod components;

// Misc shared core pieces
mod core;

// Small util bits
mod util;

// Various checks
mod check;

// Loading up info about the system
mod info;

// The state of an in-progress upgrade
mod state;

// Dealing with the f-u servers themselves
mod server;

// The update metadata files
mod metadata;


// CLI Commands
mod cmd;
