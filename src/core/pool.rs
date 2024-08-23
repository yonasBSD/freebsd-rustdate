//! Genericized threadpool.  This will get used to setup specialized
//! pools for various parallel work we want to do, like HTTP fetches and
//! filesystem scans.


// Most impl's will get put under here.

/// HTTP fetching
pub(crate) mod fetch;

/// Filesystem scanner, loading up info about the system
pub(crate) mod scan;

/// Stashing files from the system into filesdir
pub(crate) mod stash;

/// Hash checking: check hashes and store away a set of hashfiles.
pub(crate) mod hashcheck;

/// bspatch'ing
pub(crate) mod patch;


// Settings for parallelism level.  Really, this is config/command-line
// stuff, but quite often pool setup is a long way removed from having
// that, so we'll just stash up info globally.  Of course, Rust doesn't
// love that for mostly good reasons, but it seems like using atomics
// works, and we only need little numbers anyway, so...
use std::sync::atomic::{AtomicU32, Ordering};

/// How many threads to use on network worloads; e.g., HTTP fetching.
static JOBS_NET: AtomicU32 = AtomicU32::new(4);

/// How many threads to use on more CPU-bound tasks, like hash checking.
/// We also use this for filesystem scanning sort of things, which are
/// technically more IO bound, but...
static JOBS_CPU: AtomicU32 = AtomicU32::new(4);

/// Read the network job limit
fn jobs_net() -> u32 { JOBS_NET.load(Ordering::Relaxed) }
/// Read the CPU job limit
fn jobs_cpu() -> u32 { JOBS_CPU.load(Ordering::Relaxed) }


/// Initialize parallelism levels.  This is expected to just get called
/// once up-front.  If None is passed for either, they'll be initialized
/// with a default value.
///
/// The default for network parallelism is 4; cranking it may be useful
/// with high bandwidth and latency, or reducing it if you're limited on
/// bandwidth or server load.
///
/// The default for CPU parallelism is the number of CPU's, up to a
/// default max of 6.  Because this is both CPU and IO in some ways, if
/// you have a high-latency IO subsystem and a single CPU, it may still
/// be useful to have a >1 value in here.  If you have a lot of CPU's but
/// slow IO, higher values may be less useful.  The default cap of 6 is
/// because it's probably fast enough for most uses, and you might want
/// your system to not be swamped out by this, but hey, you do you.
pub(crate) fn init_jobs(net: &Option<u32>, cpu: &Option<u32>)
{
	let newnet = net.unwrap_or(4);
	let newcpu = match cpu {
		Some(c) => *c,
		None => {
			let def: std::num::NonZeroUsize = 1.try_into().unwrap();
			let def: Result<_, std::io::Error> = Ok(def);
			let mut ncpu = std::thread::available_parallelism().or(def)
					.unwrap().get().try_into().unwrap();
			if ncpu > 6 { ncpu = 6; }
			ncpu
		},
	};

	// Guard against somebody setting 0
	if newnet < 1 { panic!("{newnet} network threads is insane."); }
	if newcpu < 1 { panic!("{newcpu} cpu threads is insane."); }

	JOBS_NET.store(newnet, Ordering::Relaxed);
	JOBS_CPU.store(newcpu, Ordering::Relaxed);
}




/// The overarching trait that implements pools.  Individual users will
/// need to define a bunch of these types as appropriate for them, and
/// fill in functions that do the steps of the process that vary.
pub(crate) trait Pool: Sized
{
	/// The finalized returned into.  This may be as simple as a
	/// Vec<Self::WorkResult>, but often will have some post-processing
	/// done by Self::finalize()
	type PoolResult;

	/// General data that will be needed for the pool in a particular
	/// instance.  e.g., the HTTP fetcher needs the agent for the
	/// HTTP-speaking, the FS scanner will need to know what basedir the
	/// paths to scan are under.  This is used to construct the
	/// UnitControl passed to each worker via the mk_unitcontrol()
	/// function.
	type Control;

	/// Data that will be passed through to the function for an
	/// individual work unit processing.  This will generally be a subset
	/// of Self::Control, since that's what it'll be made from.
	/// Actually, it will usually just be the same structure.  I'm
	/// uncertain as to whether this extra layer will really be needed in
	/// practice.
	type UnitControl: Send;

	/// A single UnitControl will be needed by each worker, so we need
	/// some way to create it from the Control.  Thus far, the structs
	/// are always the same thing, and the implementation of this is just
	/// calling .clone().  As we get more done with things, it seems
	/// likely that we'll collapse away the distinction and just use
	/// Control and clone it.
	fn mk_unitcontrol(ctrl: &Self::Control) -> Self::UnitControl;


	/// Each worker will recieve an individual unit of work, as a request
	/// that gets run with.
	type WorkRequest: Send + Sync + 'static;
	/// Each worker will process each WorkRequest that into some sort of
	/// result that it'll return up to be aggregated.
	type WorkResult: Send;
	/// A worker may retrun an error for a given WorkRequest.
	type WorkErr: Send;

	/// Individual worker runner; each thread will dispatch one of these
	/// for each unit of work.  It will receive the general
	/// Self::UnitControl for whatever common information it needs to do
	/// its work, and a single Self::WorkRequest for the particular piece
	/// of work to do.  Then it'll return a WorkResult of "success" or a
	/// WorkErr of "error" (however those may be defined in your
	/// particular case.
	///
	/// e.g., if you're scanning a list of files with 4 threads, each
	/// thread will pick up the next file in the list, and call this
	/// function with it as the req.  Then it'll do whatever it's
	/// scanning for on that file, and return a WorkResult of "here's the
	/// info about the file", or a WorkErr of "I can't process that
	/// file".
	fn work(ctrl: &Self::UnitControl, req: Self::WorkRequest)
			-> Result<Self::WorkResult, Self::WorkErr>;


	/// The result of each work unit may need processing as they come in,
	/// to aggregate them together somehow.  This will get called for
	/// each Self::work()'s return, so it will neeed to handle Ok/Err as
	/// appropriate.  Generally, this will be accumulating them up into
	/// some sort of internal Vec's or the like in the impl'ing struct.
	fn work_result(&mut self, resp: Result<Self::WorkResult, Self::WorkErr>);


	/// Finalizer: this is called after all the results have come in, to
	/// prepare the final return.  That is, every Self::WorkRequest has
	/// been dispatched to a Self::work(), the result has been processed
	/// by Self::work_result(), and the threads have been spun down.
	/// This will craft the Self::PoolResult that gets returned.
	fn finalize(self) -> Self::PoolResult;


	/// How many threads to spin off.  The default is probably a decent
	/// starting guess, but it's fair that different uses may want
	/// different numbers.  e.g., if you're SHA256'ing a bunch of files,
	/// you probably don't need more threads than cores, but if you're
	/// doing a lot of HTTP requests, they're presumably gonna be idle a
	/// lot.
	///
	/// Individual pool impl's are recommended to wrap jobs_network() or
	/// jobs_cpu() as appropriate, unless they really know better.
	fn nthreads(&self) -> u32 { 4 }


	/// The main runner.  This is the provided func that will tie all the
	/// above pieces together.  It will return the info that
	/// Self::finalize() built in the returned Result.  An error return
	/// from here is only an error from the implementation; there's no
	/// way for e.g. an individual worker to halt things in the middle
	/// and return an error.  The individual impl can only control what's
	/// in the Self::PoolResult.
	fn run(mut self, ctrl: &Self::Control, items: Vec<Self::WorkRequest>)
			-> Result<Self::PoolResult, anyhow::Error>
	{
		// Spawn off a thread scope for all the fun details
		std::thread::scope(|s|
				-> Result<Self::PoolResult, anyhow::Error> {

			// Prep channels for passing requests and results around.
			use crossbeam::channel;
			let (req_snd, req_rcv) = channel::unbounded();
			let (res_snd, res_rcv) = channel::unbounded();

			// Spawn off the threadpool
			let nthr = self.nthreads();
			if nthr == 0 { panic!("nthreads {nthr} is insane"); }
			for _ in 1..=nthr
			{
				let uctrl = Self::mk_unitcontrol(&ctrl);
				let reqs = req_rcv.clone();
				let ress = res_snd.clone();
				s.spawn(move || {
					// Loop over requests until we run out
					while let Ok(req) = reqs.recv()
					{
						let res = Self::work(&uctrl, req);
						// Should be impossible for send to fail; that'd
						// only happen if the response channel were
						// closed
						ress.send(res)
								.expect("Response channel shouldn't be closed");
					}

					// Will fall off the end when the reqs channel is
					// closed, which means every piece of work has been
					// sent, and we've run out of stuff todo.
				});
			}

			// Only ref's to these channels should be down in the workers
			// now.
			drop(req_rcv);
			drop(res_snd);


			// Now feed in all the work items
			for i in items.into_iter()
			{
				// I'm a little unclear on why it needs 'static to send
				// an owned value in a scoped thread...  the individual
				// impl's were just fine with creating and sending on the
				// fly.
				req_snd.send(i)?;
			}

			// Now we've sent all the work to do, so get rid of our
			// sending channel; that will let the workerss all silently
			// fall out of their receive loops when there's nothing left
			// to do.
			drop(req_snd);


			// Now call the impl'ers function to process the results as
			// they come in.
			while let Ok(resp) = res_rcv.recv()
			{
				self.work_result(resp);
			}

			// Call the finalizer, and that's what we give back.
			let ret = self.finalize();
			Ok(ret)
		})
	}
}
