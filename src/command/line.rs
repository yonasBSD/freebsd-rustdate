//! Command line handling
//!
//! General invocation:
//! $0 [options] <command> [command-opts]

use clap::{Parser, Subcommand};
use std::path::PathBuf;

// Add extra default'ing to make config tests easier

/// Main arg entry point
#[cfg_attr(test, derive(Default))]
#[derive(Debug)]
#[derive(Parser)]
#[command(about = "Upgrade your FreeBSD system.  Today.")]
#[command(version)]
pub struct FrArgs
{
	#[command(subcommand)]
	pub(crate) command: FrCmds,

	/// Config file
	#[arg(short, long, default_value="/etc/freebsd-update.conf")]
	pub(crate) config: PathBuf,

	/// Act as if we're running a given version.
	///
	/// This probably has limited uses.  You would probably only need to
	/// specify it if we can't figure out the version normally, which we
	/// usually can.  Even with a `--basedir`, if we can run that
	/// basedir's `freebsd-version`, we should get the right answer.
	#[arg(id="as-version", long)]
	pub(crate) fakeversion: Option<String>,

	/// How many CPU-bound threads to run in parallel
	/// (default numcpu up to 6).
	///
	/// This affects local CPU-bound tasks (like compression and
	/// hashing), and also some more IO-bound tasks (like filesystem
	/// scanning).  The two are often pretty tightly coupled anyway.
	/// With a fast IO subsystem and lots of CPUs, it may sometimes be
	/// useful to raise this, but the default is probably pretty fast
	/// anyway.
	#[arg(short='j', long)]
	pub(crate) jobs_cpu: Option<u32>,

	/// How many network request to do in parallel (default 4).
	///
	/// This affects fetching files from the freebsd-update server.
	/// Raising this _may_ speed things up, but also may not, and will
	/// add to server load.  If you're running your own server, and it's
	/// got lots of bandwidth and high latency, you may gain from raising
	/// this, but generally you should leave it alone.
	#[arg(short='J', long)]
	pub(crate) jobs_net: Option<u32>,


	// Some config file params can be overriden on the command line

	/// Operate on system mounted on a given basedir.
	///
	/// By default, we operate on `/`, which is to say we upgrade the
	/// system you're running on.  This is useful if you have a full
	/// system on a subdir to work with.  For instance, if it's a jail,
	/// or you're building a system to tar up or make an image of.
	#[arg(short, long)]
	pub(crate) basedir: Option<PathBuf>,

	/// Store working files in workdir.
	///
	/// This is where we store the state of things, and also any files we
	/// download or locally stash.
	///
	/// Note that we store state seperately for each `--basedir`, so
	/// sharing a `--workdir` between multiple basedirs won't cause any
	/// functional issues.  And will probably save you some duplicate
	/// work.
	#[arg(short, long)]
	pub(crate) workdir: Option<PathBuf>,

	/// Server to fetch updates from.
	///
	/// By default, the FreeBSD Project's freebsd-update infrastructure
	/// will be used.  You probably don't need to change this unless
	/// you're running your own server (either self-build, or a local
	/// HTTP cache).
	#[arg(id="server", short, long)]
	pub(crate) servername: Option<String>,
}



/// Individual subcommands and their args
#[cfg_attr(test, derive(Default))]
#[derive(Debug)]
#[derive(Subcommand)]
pub(crate) enum FrCmds
{
	/// Dummy value (mostly to make derive(Default) happy...)
	#[cfg(test)]
	#[cfg_attr(test, default)]
	#[command(skip)]
	Dummy,

	/// Fetch updates to current version.
	///
	/// This is used to update to later patches of a given release.
	/// e.g., from 1.3-RELEASE-p1 to 1.3-RELEASE-p3.
	Fetch(FrCmdFetch),

	/// Fetch updates to current version via cron.
	///
	/// This should be used to automate running a regular `fetch` from a
	/// cronjob.  It produces no direct output unless it hits an error.
	/// If it finds pending updates that need to be installed, it sends
	/// the output to the `MailTo` config param.
	Cron(FrCmdCron),

	/// Fetch upgrades to a new version.
	///
	/// This is used to upgrade to newer releases.  e.g., from
	/// 1.3-RELEASE to 1.4-RELEASE.  Note that generally, upgrading to a
	/// new version will require rebooting into the new kernel before
	/// installing the new world; the `install` command will operate in
	/// individual steps in this case.
	Upgrade(FrCmdUpgrade),

	/// Install downloaded updates or upgrades.
	///
	/// This is used after a `fetch` or `upgrade` to actually install the
	/// changes to the running system.  If this is a cross-version
	/// `upgrade`, then `install` may need to be run multiple times to do
	/// the individual steps of the upgrade.
	Install(FrCmdInstall),

	/// Show information about a pending install.
	///
	/// This will give a summary of what the install will do, in terms of
	/// creating new files, removing old files, updating changed files,
	/// etc.  Using the `-v` argument will allow you to display details
	/// of one (or multiple) of those, like the list of the actual
	/// path[s] that will be touched.
	///
	/// If no install is pending, it will just tell you that.
	ShowInstall(FrCmdShowInstall),

	/// Show merged files from a pending upgrade.
	///
	/// In the case of a cross-version `upgrade`, locally changed config
	/// files may have their differences merged (see the `MergeChanges`
	/// config param).  This will let you see what the differences as a
	/// result of those merges are.
	///
	/// By default, the diffs will be relative to your currently
	/// installed file, so will show the changes that will be put in
	/// place as a result of calling `install`.  With the `-u` argument,
	/// you can show the diffs against the new upstream version instead,
	/// so will show roughly "your local changes".
	///
	/// If there are merge conflicts you haven't resolved yet, this
	/// command will also mention it; see the `resolve-merges` command to
	/// resolve them.
	ShowMerges(FrCmdShowMerges),

	/// Resolve conflicted merges for a pending upgrade.
	///
	/// In the case of a cross-version `upgrade`, locally changed config
	/// files may have their differences merged.  If the merge attempt
	/// fails, you'll have to manually resolve those conflicts.  This
	/// command will walk you through all the conflicted files, drop you
	/// into your `$EDITOR` with the conflicts, and let you resolve them.
	/// You may then choose to accept your resolution or skip it (to
	/// retry later).
	///
	/// If there are unresolved merges, `install` will refuse to proceed,
	/// and `show-install` will also tell you about it.  Calling this
	/// command with the `-e` will will non-interactively tell you if
	/// there are pending conflicts to be resolved.
	ResolveMerges(FrCmdResolveMerges),

	/// Clean up stuff (not all stuff included).
	///
	/// This requires an argument for what to clean.  Currently pending
	/// update/upgrade info is the only implemented feature.  It might be
	/// nice if this could clean up old downloaded/cached files in a
	/// smart way someday...
	Clean(FrCmdClean),

	/// Check current system state against upstream expectation.
	///
	/// This provides a summary of how your running system differs from a
	/// "clean" system of the current version.  This is naturally a
	/// somewhat limited and quirky thing to do; on any real system,
	/// you've probably changed some config files, which means they'll be
	/// showing up here.  So this should be used with a little caution,
	/// and its results should definitely by considered as possibly
	/// useful information for a human, not as any sort of list of
	/// "broken" things.
	///
	/// Because of how freebsd-update works, this will give bad
	/// information if you're not currently caught up to the latest
	/// patchlevel if your version, so you should be `fetch`'d up to date
	/// before running this.
	///
	/// You can limit the results to certain paths by using `--paths` or
	/// `--exclude`; both take regular expressions.
	///
	/// Using the `--ignore` arg (possibly multiple times) lets you
	/// ignore certain types of differences.  For example, if you're
	/// running as a regular user against a `--basedir` you own, you can
	/// assume that the `uid`, `gid`, and `flags` will always differ from
	/// upstream, so you'd probably want `-i uid,gid,flags` to always be
	/// given.
	///
	/// Also notable is the SHA256 hashes.  If you ignore those
	/// differences (`-i hash`), then this command doesn't need to SHA256
	/// your whole system to do the checks, so it will run a lot faster.
	/// If you're caring mostly about all the files existing, being the
	/// right type, and having reasonable permissions, this will get you
	/// the answers a lot quicker.
	///
	/// This can be used in some similar ways to `freebsd-update.sh`'s
	/// `IDS` command.  However, trying to use it as an IDS has so many
	/// limitations and caveats, that we choose not to even name it in
	/// such a ways as to pretend.
	CheckSys(FrCmdCheckSys),

	/// Quick check of whether there might be a newer patch available.
	///
	/// This does a cursory comparison of your system's patchlevel to the
	/// latest patchlevel available from the server.  It won't
	/// necessarily be as reliable in edge cases as attempting a full
	/// `fetch` to see if there's anything to get.  However, it's
	/// **much** cheaper, for both you and the server, so it's a good
	/// quick check to see if you've fallen behind.
	///
	/// When run from cron, use the `--cron` arg to to spread out load.
	/// And you probably want to run with `-q`, so it only produces
	/// output (and would thus cause cron to mail you) when there's a new
	/// patch waiting.
	CheckFetch(FrCmdCheckFetch),

	/// Extract a file or subtree exactly from upstream.
	///
	/// Calling this with a path or several paths (possibly expressed as
	/// a regular expression; see `-x`) will grab pristine copies of the
	/// given paths from upstream, and apply them to your running system.
	/// Note that it uses exact matches of the paths, so if you give it a
	/// directory, it will only affect that directory, not its contents.
	/// Using regex matches is probably how you want to go if you want to
	/// act on a whole subtree.
	///
	/// This can be useful if `check-sys` shows you differences you don't
	/// expect, and you want to just blat the pristine upstream over
	/// something.  If you're doing large-scale extraction (like trying
	/// to use it to extract the whole source tree), this will be
	/// extremely inefficient for you, and probably very heavy on the
	/// server, so go another route like using release tarballs.
	///
	/// However, if you have some spot differences ("Ooops, I
	/// accidentally ran `rm /boot/kernel/kernel`!"), this can let you
	/// paper over them real quick before anyone notices.
	///
	/// **PAY ATTENTION**; this is an extremely powerful command, and you
	/// can very quickly do a lot of damage if you're not careful.  It's
	/// hard to recommend strongly enough that you make copious use of
	/// `--dry-run` to be sure this will do _exactly_ what you want, and
	/// to be _extremely_ cautious about pulling out `--force`.
	Extract(FrCmdExtract),

	/// Dump out metadata info for a version.  (DEV)
	///
	/// This is of no interest to anybody who's not working on
	/// freebsd-rustdate's code.  It just automates fetching down the
	/// metadata files from the server for a particular version and
	/// extracting them somewhere, so you can manually poke at things.
	#[clap(hide(true))]
	DumpMetadata(FrCmdDumpMetadata),
}



/*
 * Individual [sub]command args
 */

/// Fetch args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdFetch
{
	/// Run as a backend via `$0 cron`.
	#[clap(hide(true))]
	#[arg(long)]
	pub(crate) as_cron: bool,

	// XXX IF we grow more here, we presumably need to add them to
	// FrCmdCron too, and adjust the cron::run() func to copy them over
	// when it re-execs.
}

/// Cron args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdCron
{
	/// Run without delay.  This is a hidden option to make dev easier.
	#[clap(hide(true))]
	#[arg(long)]
	pub(crate) immediately: bool,
}

/// Upgrade args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdUpgrade
{
	/// Release to upgrade to (e.g., 13.2-RELEASE)
	#[arg(short, long)]
	pub(crate) release: crate::info::version::AVersion,
}

/// Install args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdInstall
{
	/// Just say what we'd do, don't actually do it.
	///
	/// If installing a cross-version upgrade, you may want to use
	/// `--all` with this to actually show all the steps, not just what
	/// will happen next.
	#[arg(verbatim_doc_comment)]
	#[arg(short='n', long)]
	pub(crate) dry_run: bool,

	/// Proceed through all steps of installing an Upgrade.
	///
	/// e.g., don't pause for reboot before installing world, etc.  This
	/// is very dangerous when you're upgrading the system you're running
	/// on, but useful to e.g. upgrade a jail from the host or the like,
	/// in one swell foop.
	#[arg(short='a', long)]
	pub(crate) all: bool,

	/// Don't fsync() files during the install.
	///
	/// This is faster, particularly on slow media or with lots of files
	/// (e.g., you have the source tree installed).  It's also more
	/// dangerous if a crash happens during or immediately after the
	/// process.
	#[arg(short='s', long)]
	pub(crate) no_sync: bool,
}

/// ShowInstall verbose types
#[derive(Debug, Clone, Eq, PartialEq, Default)]
#[derive(clap::ValueEnum)]
#[derive(strum::Display, strum::EnumString, strum::AsRefStr)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum ShowInstallType
{
	/// All types
	#[default]
	All,

	/// Added files
	Add,

	/// Removed files
	#[strum(serialize = "remove")]
	Rm,

	/// Updated files
	Update,

	/// Paths with changed type
	Change,

	/// Merged files
	Merge,
}

/// ShowInstall args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdShowInstall
{
	/// Show full details (e.g., file lists).
	#[arg(short, long, value_delimiter = ',', num_args = 1..)]
	pub(crate) verbose: Vec<ShowInstallType>,
	// XXX Don't seem to be able to talk clap into accepting "-v with no
	// arg -> ::All", sigh.
}

/// ShowMerges args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdShowMerges
{
	/// Show diffs relative to new upstream instead of your current file.
	///
	/// This is useful to see something more like "what changes have I
	/// made", vs. "what changes will running install put onto my
	/// system".
	#[arg(short, long)]
	pub(crate) upstream: bool,
}

/// ResolveMerges args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdResolveMerges
{
	/// Don't attempt resolution, just describe our state and exit.
	///
	/// If there are no conflicts to resolve, this will just exit(0).  If
	/// there are, it'll say how many there are, then exit non-zero.
	#[arg(short, long)]
	pub(crate) exit: bool,
}

/// DumpMetadata args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdDumpMetadata
{
	/// Version to dump metadata for (e.g., 13.2-RELEASE)
	#[arg(short, long)]
	pub(crate) version: crate::info::version::AVersion,

	/// Directory to save the files into (must exist)
	#[arg(short, long)]
	pub(crate) dir: PathBuf,
}

/// Clean args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdClean
{
	/// Clean up info about a pending update (i.e., erase knowledge of
	/// a previous `fetch` or `upgrade`).
	#[arg(short, long)]
	pub(crate) pending: bool,
}

/// CheckSys diff-ignore types
#[derive(Debug, Clone, Eq, PartialEq)]
#[derive(clap::ValueEnum)]
#[derive(strum::Display, strum::EnumString, strum::AsRefStr)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum CheckSysIgnore
{
	// These match the names for the MetdataLineDiff types
	/// Owning user
	Uid,

	/// Owning group
	Gid,

	/// Mode
	Mode,

	/// Flags
	Flags,

	/// File hash
	Hash,

	/// Link target
	Target,

	// These are other bits
	/// Missing files
	Missing,

	/// Mismatched file types
	Type,
}

/// CheckSys args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdCheckSys
{
	/// Ignore differences of some type (can be specified multiple
	/// times).
	///
	/// Notably, if `hash` diffs are ignored, there's no need to hash the
	/// files on your system for comparison, so this will run
	/// significantly faster.
	#[arg(short, long, value_delimiter = ',', num_args = 1..)]
	pub(crate) ignore: Vec<CheckSysIgnore>,

	/// Include only matching paths (regex)
	#[arg(short, long)]
	pub(crate) paths: Vec<regex_lite::Regex>,

	/// Exclude paths (regex)
	#[arg(short = 'x', long)]
	pub(crate) exclude: Vec<regex_lite::Regex>,
}

/// CheckFetch args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdCheckFetch
{
	/// Run quietly.
	///
	/// Don't output anything if there's no update apparent.  If
	/// specified twice, no output will be given in any case; a non-zero
	/// exit will signal a newer patch being present.
	#[arg(short, long, action = clap::ArgAction::Count)]
	pub(crate) quiet: u8,

	/// Add a random delay to run via cron
	///
	/// This delays some random amount up to an hour, to spread the runs
	/// out when run via cron.
	#[arg(short, long)]
	pub(crate) cron: bool,
}

/// Extract args
#[derive(Debug)]
#[derive(Parser)]
pub(crate) struct FrCmdExtract
{
	/// Just say what we'd do, don't actually do it.
	///
	/// Use this a lot, to be _very_ sure a given extract will do
	/// _exactly_ what you want.
	#[arg(short='n', long)]
	pub(crate) dry_run: bool,

	/// Treat given paths as regular expressions instead of literals.
	///
	/// Using this (probably with a ^-anchor) is probably the easiest way
	/// to specify a full subtree.
	#[arg(short='x', long)]
	pub(crate) regex: bool,

	/// Attempt to filter down to installed components.
	///
	/// By default we'll extract anything that matches the given path
	/// patterns.  With this, like `upgrade`, we'll attempt to prune down
	/// the default list of components to just those you have installed.
	#[arg(short='c', long)]
	pub(crate) only_components: bool,

	/// Forcibly overwrite existing files, even if they look the same.
	#[arg(short, long)]
	pub(crate) force: bool,

	/// Some number of path[s] to work with.
	///
	/// If `-x` is given, these are treated as regular expressions.
	/// Otherwise they're expected to be exact string matches.
	pub(crate) paths: Vec<std::ffi::OsString>,
}




/*
 * Misc impls and utils
 */

impl std::fmt::Display for FrCmds
{
	fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error>
	{
		match self
		{
			Self::Fetch{..}   => f.write_str("fetch"),
			Self::Cron{..}    => f.write_str("cron"),
			Self::Upgrade{..} => f.write_str("upgrade"),
			Self::Install{..} => f.write_str("install"),
			Self::Clean{..}   => f.write_str("show-install"),
			Self::Extract{..} => f.write_str("extract"),
			Self::CheckSys{..}    => f.write_str("check-sys"),
			Self::CheckFetch{..}  => f.write_str("check-fetch"),
			Self::ShowMerges{..}  => f.write_str("show-merges"),
			Self::ShowInstall{..} => f.write_str("show-install"),
			Self::ResolveMerges{..} => f.write_str("resolve-merges"),

			// More dev/debug-ish stuff
			Self::DumpMetadata{..} => f.write_str("dump-metadata"),

			// Shouldn't really be possible
			#[cfg(test)]
			Self::Dummy => f.write_str("dummy"),
		}
	}
}


impl FrArgs
{
	/// Build up the [global] args passed to us, to duplicate ourself.
	/// We use this for the `cron` command, to re-exec fetch.
	///
	/// Technically we really want OsStr's for process::Command here, but
	/// String's are easier to make.  This will work out badly if people
	/// are passing args that aren't UTF8-able, but I'll worry about that
	/// when it happens.
	pub(crate) fn mk_args(&self) -> Vec<String>
	{
		let mut ret = Vec::new();

		// Config defaults, so there's always a value.  It may be
		// redundant, but what the heck...
		ret.push(format!("--config={}", self.config.to_str().unwrap()));

		if let Some(v) = &self.fakeversion
		{ ret.push(format!("--as-version={v}")); }
		if let Some(v) = &self.jobs_cpu
		{ ret.push(format!("--jobs-cpu={v}")); }
		if let Some(v) = &self.jobs_net
		{ ret.push(format!("--jobs-net={v}")); }
		if let Some(v) = &self.servername
		{ ret.push(format!("--server={v}")); }

		// There are paths, so assume they can str-ify like we did with
		// config.
		if let Some(p) = &self.basedir
		{ ret.push(format!("--basedir={}", p.to_str().unwrap())); }
		if let Some(p) = &self.workdir
		{ ret.push(format!("--workdir={}", p.to_str().unwrap())); }

		ret
	}
}



pub fn parse() -> FrArgs
{
	let ret = FrArgs::parse();

	// Setup the parallelism bits from the parse
	crate::core::pool::init_jobs(&ret.jobs_net, &ret.jobs_cpu);

	ret
}
