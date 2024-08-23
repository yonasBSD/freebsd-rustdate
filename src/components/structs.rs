//! Component-handling structs and their bits.


/// Top-level component
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(strum::Display, strum::EnumString, strum::AsRefStr)]
// #[derive(strum::EnumIs)]
pub(crate) enum BaseComponent
{
	#[strum(serialize = "kernel")]
	Kernel,
	#[strum(serialize = "src")]
	Src,
	#[strum(serialize = "world")]
	World,
}


/// A subcomponent.  Not all combinations of this and Component make
/// sense, but I'm not gonna try overmodelling too much until I have a
/// good reason to...
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(strum::Display, strum::EnumString, strum::AsRefStr)]
// #[derive(strum::EnumIs)]
pub(crate) enum BaseSubComponent
{
	// kernel choices
	#[strum(serialize = "generic")]
	Generic,
	#[strum(serialize = "generic-dbg")]
	GenericDbg,

	// src only normally has 1
	#[strum(serialize = "src")]
	Src,

	// world has a few
	#[strum(serialize = "base")]
	Base,
	#[strum(serialize = "base-dbg")]
	BaseDbg,
	#[strum(serialize = "lib32")]
	Lib32,
	#[strum(serialize = "lib32-dbg")]
	Lib32Dbg,

	// We could leave this a bit more open, but this doesn't work with
	// strum going back to a string; it won't format a tuple value, and
	// it won't FromStr a named value.
	// Rel 0.25 allegedly allowed doing this by setting no to_string() on
	// this, but that only makes `variant.to_string()`, DTRT; if you have
	// it somewhere .as_ref() or the like it still just calls it Other.
	// #[strum(to_string = "{0}")]
	// #[strum(default)]
	// Other(String),
}


/// A component entry of some sort, including both the Component and the
/// Subcomponent
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Component
{
	/// The component
	pub(crate) comp: BaseComponent,

	/// May be a subcomponent, or maybe not?
	pub(crate) subcomp: Option<BaseSubComponent>,
}

impl std::fmt::Display for Component
{
	fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error>
	{
		match &self.subcomp
		{
			Some(sc) => write!(f, "{}/{}", self.comp, sc),
			None     => write!(f, "{}", self.comp),
		}
	}
}

impl std::str::FromStr for Component
{
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err>
	{
		let mut spl = s.split('/');
		let comp = spl.next().ok_or_else(|| "No component".to_string())?;
		let subcomp = spl.next();

		let comp = comp.parse().map_err(|e| format!("Bad component: {e}"))?;
		let subcomp = match subcomp {
			Some(s) => Some(s.parse()
					.map_err(|e| format!("Bad subcomponent: {e}"))?),
			None    => None,
		};

		let ret = Component { comp, subcomp };
		Ok(ret)
	}
}




#[cfg(test)]
mod tests
{
	#[test]
	fn bcomp_strs()
	{
		use super::BaseComponent as BC;

		let tst = [
			("kernel", BC::Kernel),
			("src",    BC::Src),
			("world",  BC::World),
		];
		for (sval, eval) in tst
		{
			let parsed: BC = sval.parse().unwrap();
			let strung = parsed.to_string();
			assert_eq!(strung, sval, "str->Enum->str OK");

			let strung2 = eval.to_string();
			assert_eq!(strung, strung2, "str->Enum->str == Enum->str");

			assert_eq!(eval.as_ref(), sval, "AsRef<str>");
		}

		let _goterr = "invalid".parse::<BC>().unwrap_err();
	}


	#[test]
	fn bsubcomp_strs()
	{
		use super::BaseSubComponent as BSC;

		// OK, I won't bother trying all of them...
		let tst = [
			("generic",  BSC::Generic),
			("base-dbg", BSC::BaseDbg),
		];
		for (sval, eval) in tst
		{
			let parsed: BSC = sval.parse().unwrap();
			let strung = parsed.to_string();
			assert_eq!(strung, sval, "str->Enum->str OK");

			let strung2 = eval.to_string();
			assert_eq!(strung, strung2, "str->Enum->str == Enum->str");

			assert_eq!(eval.as_ref(), sval, "AsRef<str>");
		}

		// And the 'other' case
		// let iparse = "invalid".parse::<BSC>().unwrap();
		// match &iparse {
		// 	BSC::Other(s) => assert_eq!(s, "invalid", "Valid invalid parse"),
		// 	x => panic!("Got {x} instead of the valid invalid!"),
		// };
		// assert_eq!(iparse.as_ref(), "invalid", "invalid -> invalid");
	}


	#[test]
	fn comp_strs()
	{
		use super::Component;

		let comp = "src".parse().unwrap();
		let subcomp = Some("src".parse().unwrap());
		let ctst = Component { comp, subcomp };
		assert_eq!(ctst.to_string(), "src/src");
		assert_eq!(ctst,             "src/src".parse().unwrap());

		let comp = "world".parse().unwrap();
		let subcomp = None;
		let ctst = Component { comp, subcomp };
		assert_eq!(ctst.to_string(), "world");
		assert_eq!(ctst,             "world".parse().unwrap());

		let comp: Component = "src/src".parse().unwrap();
		assert_eq!(comp.comp.as_ref(), "src");
		assert_eq!(comp.subcomp.as_ref().unwrap().as_ref(), "src");

		// let comp: Component = "world/idunno".parse().unwrap();
		// assert_eq!(comp.comp.as_ref(), "world");
		// assert_eq!(comp.subcomp.as_ref().unwrap().to_string(), "idunno");

		let comp: Component = "world".parse().unwrap();
		assert_eq!(comp.comp.as_ref(), "world");
		assert!(comp.subcomp.is_none());
	}
}
