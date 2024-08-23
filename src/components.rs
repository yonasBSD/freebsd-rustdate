//! Install[ed] components related bits
//!
//! There appear to only be 3 components; kernel src world.  The list of
//! subcomponents is a little less totally clear, so for the moment I'm
//! just gonna say we define the components, and wing it on
//! subcomponents...


/// Various component-related structs
mod structs;
pub(crate) use structs::Component;
pub(crate) use structs::{BaseComponent, BaseSubComponent};



impl Component
{
	/// When specifying components in the config file to care about, it's
	/// _common_ to list only the components, but _supported_ to list the
	/// componenents.  f-u.sh uses look(1) to do string compares in a way
	/// that if you specify just a component, that covers all the
	/// subcomponents, but if you specify a sub, it matches only that one.
	/// So give us a method to do that check.
	pub(crate) fn contains(&self, other: &Self) -> bool
	{
		if self.comp != other.comp { return false }
		if self.subcomp.is_none()  { return true  }
		self.subcomp == other.subcomp
	}
}



#[cfg(test)]
pub(crate) mod tests
{
	use super::Component;

	pub(crate) fn src_comp()   -> Component { "src".parse().unwrap() }
	pub(crate) fn world_comp() -> Component { "world".parse().unwrap() }
	pub(crate) fn base_comp()  -> Component { "world/base".parse().unwrap() }
	pub(crate) fn lib32_comp() -> Component { "world/lib32".parse().unwrap() }


	#[test]
	fn contains()
	{
		assert!(world_comp().contains(&lib32_comp()));
		assert!(world_comp().contains(&base_comp()));
		assert!(!lib32_comp().contains(&world_comp()));

		assert!(!src_comp().contains(&base_comp()));
		assert!(!src_comp().contains(&lib32_comp()));
		assert!(!base_comp().contains(&src_comp()));
	}
}
