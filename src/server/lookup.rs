//! Looking up the list of servers
use super::Server;


/// Figure out our whole list of servers, based on the given name.
///
/// This is the main external entry point, that the rest of the code hits
/// to put together the list of servers to try reaching out to.
pub(crate) fn servers(sname: &str) -> Result<Vec<Server>, anyhow::Error>
{
	// Let's see what we get outta DNS...
	let srvs = match srv_lookup(sname)? {
		Some(srvs) => srvs,
		None => {
			// OK, time to fake a single "just this name".  Also this
			// means the rest of our processing is meaningless, so we
			// might as well just go ahead and return it...
			let nsrv = Server {
				host: sname.to_string(),
				..Server::default()
			};
			return Ok(vec![nsrv]);
		}
	};

	// Bust it out into pieces by priority
	let mut srvs = srvs_by_pri(srvs);

	// Shuffle it around by weight within each priority
	shuffle_weights(&mut srvs);

	// And now we can flatten it all the way back down to just a "try in
	// this order" list.
	let srvs = srvs.into_iter().flatten().collect();

	// And that's it
	Ok(srvs)
}





/*
 * The rest of this is just internal implementation details of the
 * external entries above.
 */



/// Internal helper: Do the SRV lookup for a name
fn srv_lookup(sname: &str) -> Result<Option<Vec<Server>>, anyhow::Error>
{
	use hickory_resolver::Resolver;
	let resolver = Resolver::from_system_conf()?;

	// Let's see what we get...
	let srvname = format!("_http._tcp.{}", sname);
	let res = match resolver.srv_lookup(srvname) {
		Ok(recs) => recs,
		Err(e) => {
			// Docs are a little scanty, but if looks like going by the
			// Kind for a NoRecordsFound would be the way we quantify "I
			// was told there's nothing" (in which case we "succeed" at
			// getting nothing) from "DNS went wonky" (in which case we
			// fail harder).  Only in the "we know there's no SRV record
			// like this" case would we be falling back in the code to
			// "OK, I guess it's just a http server itself".
			use hickory_resolver::error::ResolveErrorKind as REK;
			match e.kind()
			{
				REK::NoRecordsFound{..} => return Ok(None),
				_ => return Err(e.into()),
			};
		},
	};

	// Roll it up
	let srvs: Vec<Server> = res.iter()
		.map(|sr| {
			let pri = sr.priority();
			let weight = sr.weight();
			let host = sr.target().to_utf8();
			Server {
				pri, weight, host,
				..Server::default()
			}
		})
		.collect();

	Ok(Some(srvs))
}


/// Internal helper: break out into a Vec<Vec<>> by priority.
///
/// Really an internal thing here, but it does get used in the tests a
/// level up, so give it a little extra visibility.
pub(super) fn srvs_by_pri(mut srvs: Vec<Server>) -> Vec<Vec<Server>>
{
	// Put it in sorted order, then we can walk down it accumulating
	// up a vec of each priority as we go.
	srvs.sort();
	let mut ret = vec![];
	let mut onepri = vec![];
	let mut lastpri = 0;
	for srv in srvs
	{
		// If moving to a new priority, stash up the previous list
		if srv.pri != lastpri
		{
			if onepri.len() > 0
			{
				ret.push(onepri);
				onepri = vec![];
			}
			lastpri = srv.pri;
		}

		// Otherwise this is another entry on the current priority
		onepri.push(srv);
	}

	// And we fell off the end
	if onepri.len() > 0 { ret.push(onepri); }

	ret
}


/// Rearrange Server's so each priority has its entries shuffled around
/// based on the weights.
///
/// A smaller RNG crate would do just as well for our purposes, but other
/// depends are already bringing in rand, so what the heck...
fn shuffle_weights(srvs: &mut Vec<Vec<Server>>)
{
	use rand::SeedableRng;
	let rng = rand_pcg::Pcg64::from_entropy();
	shuffle_weights_be(srvs, rng);
}

// Separated out into 2 funcs so we can thunk in for testing
fn shuffle_weights_be(srvs: &mut Vec<Vec<Server>>, mut rng: impl rand::Rng)
{
	// Should probably do this in place, but heck with it...
	for pri in 0..srvs.len()
	{
		let shuffled = shuffle_weight(&mut srvs[pri], &mut rng);
		srvs[pri] = shuffled;
	}
}

fn shuffle_weight(srvs: &mut Vec<Server>, rng: &mut impl rand::Rng)
		-> Vec<Server>
{
	let mut ret = Vec::with_capacity(srvs.len());

	// Cheat: if there's only 1 entry, a random permutation is the one
	// entry...
	if srvs.len() == 1
	{
		ret.push(srvs.remove(0));
		return ret;
	}

	let mut cws = std::collections::VecDeque::with_capacity(srvs.len());
	while srvs.len() > 0
	{
		// RFC2782 algorithm.  They're already sorted, so we can just
		// accumulate up the counters for cumulative weight, then pick a
		// number in that range.
		let mut sum: u32 = 0;
		cws.clear();
		for i in 0..srvs.len()
		{
			cws.push_front((sum, i));
			sum += srvs[i].weight as u32;
		}
		let rn = rng.gen_range(0..sum);
		for sent in &cws
		{
			if sent.0 < rn
			{
				ret.push(srvs.remove(sent.1));
				break;
			}
		}
	}

	ret
}




#[cfg(test)]
mod tests
{
	use super::*;
	use super::super::server::tests::test_servers;


	#[test]
	fn shuffling()
	{
		// Take just the pri 3 ones, to try the weight sorting
		let mut srvs = srvs_by_pri(test_servers());
		let mut srvs = srvs.remove(1);
		srvs.retain(|s| s.pri == 3);

		// Shuffle 'em with a known seed, so the order should be
		// repeatable
		use rand::SeedableRng;
		let mut rng = rand_pcg::Pcg64::seed_from_u64(31415926u64);
		srvs = shuffle_weight(&mut srvs, &mut rng);

		// With that seed, we get jane joe barbara
		assert_eq!(srvs[0].host, "jane");
		assert_eq!(srvs[1].host, "joe");
		assert_eq!(srvs[2].host, "barbara");
	}

	#[test]
	fn shuffling_list()
	{
		// Shuffle the whole list
		let mut srvs = srvs_by_pri(test_servers());

		// With a known (different from above, just for kicks) seed
		use rand::SeedableRng;
		let mut rng = rand_pcg::Pcg64::seed_from_u64(3141592u64);
		shuffle_weights_be(&mut srvs, &mut rng);

		// pri 2/30 are just single-entry, so what could change?
		assert_eq!(srvs[0].len(), 1, "pri 2 len");
		assert_eq!(srvs[2].len(), 1, "pri 30 len");

		// That seed gives us joe jane barbara
		assert_eq!(srvs[1][0].host, "joe");
		assert_eq!(srvs[1][1].host, "jane");
		assert_eq!(srvs[1][2].host, "barbara");
	}
}
