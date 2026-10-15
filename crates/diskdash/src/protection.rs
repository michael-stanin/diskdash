//! Wiring of the central path-protection filter to the user's settings.

use diskdash_analysis::eval::protected::Protected;
use diskdash_analysis::rules::Env;

use crate::settings::Settings;

/// The protection filter for the current settings: built-in protected paths plus
/// the user's exclusions and protected folders.
pub fn build(env: &dyn Env, settings: &Settings) -> Protected {
    Protected::new(env).with_user(&settings.excluded_paths, &settings.protected_paths)
}
