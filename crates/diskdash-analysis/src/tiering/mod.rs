//! Storage tiering: which cold data could move from an SSD to an HDD, which hot
//! data from an HDD to an SSD, where it would fit, and what free space would
//! look like afterwards. Analysis only: nothing here moves or changes a file.
//! The only files read are launcher manifests (Steam, Epic), read-only.

pub mod assign;
pub mod hot;
pub mod libraries;
pub mod vdf;

pub use assign::{
    assign, project, Assignment, DriveState, Limits, Need, Projection, Tier, NO_TARGET_SPACE, NO_TARGET_TIER,
};
pub use hot::{hot_folders, HotFolder, HotParams};
pub use libraries::{epic_apps, steam_apps, Launcher, LibraryApp, ManifestSource};
