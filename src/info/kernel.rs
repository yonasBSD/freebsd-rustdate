//! Various info about the kernel


/// Build the boilerplate for the wrapper funcs
macro_rules! mk_sysctl_func {
	( $fn:ident, $sysctl:literal, $ret:path ) => {
		pub(crate) fn $fn() -> Result<$ret, anyhow::Error>
		{
			use sysctl::{Ctl, Sysctl};
			use anyhow::anyhow;

			let sv_s = Ctl::new($sysctl)
					.map_err(|e| { anyhow!("sysctl {}: {}", $sysctl, e) })?;
			let sv = sv_s.value_string()
					.map_err(|e| { anyhow!("{} string: {}", $sysctl, e) })?;

			munge::$fn(sv)
		}
	};
}

// What dir is the running kernel in?  We're assuming it's always called
// 'kernel' I guess...
mk_sysctl_func!(dir,  "kern.bootfile", String);

// What's the running kernel config?  freebsd-update.sh uses uname -i,
// which loads sysctl kern.ident
//
// I'm a little worried about this. Sysctl::value_string() really wants
// to go through rust's String, and kern.ident seems rather susceptible
// to not being UTF-8.  Though I suspect 99.999% of them are presumably
// 7-bit ASCII, so hopefully we'll never hit a problem and have to get
// extra creative...
mk_sysctl_func!(conf, "kern.ident", String);

// What's the arch?  freebsd-update.sh uses uname -m, which loads
// sysctl hw.machine.
mk_sysctl_func!(arch, "hw.machine", String);

/// Are we in a jail?
///
/// Not exactly "kernel" related, but all the sysctl stuff is here
/// already, so JFDI.  Hopping through the value_string() from our wrapper
/// above is probably just more work, though we're assumign the type...
pub(crate) fn jailed() -> Result<bool, anyhow::Error>
{
	use sysctl::{Ctl, Sysctl as _};
	use anyhow::anyhow;

	let jctl = "security.jail.jailed";
	let sv_s = Ctl::new(jctl)
			.map_err(|e| { anyhow!("sysctl {}: {}", jctl, e) })?;
	let sv = sv_s.value()
			.map_err(|e| { anyhow!("{} value: {}", jctl, e) })?;

	// Should just be an Int.
	let jailed = sv.as_int()
			.ok_or_else(|| { anyhow!("{} not int?  {:?}", jctl, sv) })?;

	Ok(jailed == &1)
}


// Mungers for the value returned from sysctl
mod munge {
	// We want just the dir the bootfile is in, not the bootfile itself.
	pub(super) fn dir(mut sv: String) -> Result<String, anyhow::Error>
	{
		let remstr = "/kernel";
		if sv.ends_with(remstr)
		{
			sv.truncate(sv.len() - remstr.len());
		}

		Ok(sv)
	}

	// According to f-u.sh, kernel config SMP is "ident SMP-GENERIC", and
	// we want the config name, so we'd tweak that here.  However,
	// AFAICS, that was an i386 config that was removed in 2003, so I'm
	// just gonna ignore the crap outta that...
	pub(super) fn conf(mut sv: String) -> Result<String, anyhow::Error>
	{
		if false { if sv == "SMP-GENERIC" { sv.truncate(3); } }

		Ok(sv)
	}

	// Nothing to do for arch
	pub(super) fn arch(sv: String) -> Result<String, anyhow::Error>
	{
		Ok(sv)
	}

	// Jailed sysctl returns an int, which our wrapper just turned into a
	// string, and then we need to bool it.  Yay.
}


#[cfg(test)]
mod tests
{
	// We bypass the sysctl layers, since those either work or don't, and
	// test the munging.
	use super::munge as m;

	#[test]
	fn dir()
	{
		// Strip
		let bootf = "/boot/kernel/kernel".to_string();
		assert_eq!(m::dir(bootf).unwrap(), "/boot/kernel");

		let bootf = "/boot/kernel.old/kernel".to_string();
		assert_eq!(m::dir(bootf).unwrap(), "/boot/kernel.old");

		// Not .../kernel, nothing to strip
		let bootf = "/boot/kernel.old/notkernel".to_string();
		assert_eq!(m::dir(bootf.clone()).unwrap(), bootf.as_str());
	}

	#[test]
	fn conf()
	{
		// Most things just pass through
		let conf = "GENERIC".to_string();
		assert_eq!(m::conf(conf.clone()).unwrap(), conf);

		let conf = "SUPERSPECIAL".to_string();
		assert_eq!(m::conf(conf.clone()).unwrap(), conf);

		// Except SMP-GENERIC turns into SMP.  x-ref comment above about
		// why we're not doing that now.
		if false
		{
			let conf = "SMP-GENERIC".to_string();
			assert_eq!(m::conf(conf).unwrap(), "SMP");
		}
	}

	#[test]
	fn jailed()
	{
		// Mostly this is just testing that jailed() has the right
		// type in the sysctl...
		let _ = super::jailed().unwrap();
	}
}
