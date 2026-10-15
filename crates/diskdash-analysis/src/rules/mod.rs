//! Cleanup rules: model, YAML parsing, loading and path expansion.

pub mod expand;
pub mod load;
pub mod model;
pub mod parse;

pub use expand::{expand, Env, MapEnv};
pub use load::{load, LoadedRule, RuleSet, UserRulesDir};
pub use model::*;
pub use parse::parse_rules;

/// Built-in rule files, embedded at build time (`rules/*.yaml` in the repo).
pub const BUILTIN: &[(&str, &str)] = &[
    ("windows.yaml", include_str!("../../../../rules/windows.yaml")),
    ("system.yaml", include_str!("../../../../rules/system.yaml")),
    ("browsers.yaml", include_str!("../../../../rules/browsers.yaml")),
    ("developer.yaml", include_str!("../../../../rules/developer.yaml")),
    ("apps.yaml", include_str!("../../../../rules/apps.yaml")),
    ("downloads.yaml", include_str!("../../../../rules/downloads.yaml")),
    ("logs.yaml", include_str!("../../../../rules/logs.yaml")),
];

/// The built-in rule set plus the user's directory (if any).
pub fn load_all(user_dir: Option<&std::path::Path>) -> RuleSet {
    load(BUILTIN, user_dir)
}
