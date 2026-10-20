//! History and trends: free-space forecast and snapshot-to-snapshot diff.
//! Pure computation over numbers and snapshot databases; nothing here touches
//! user files.

pub mod diff;
pub mod forecast;

pub use diff::{
    categories_delta, diff_dirs, load_dirs, load_summary_dirs, CategoryDelta, DiffResult, DirDelta, DirRow,
};
pub use forecast::{forecast, Forecast, ForecastLimits, Sample};
