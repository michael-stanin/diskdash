//! The report model: one stable, versioned shape that the JSON, CSV and HTML
//! writers all render. Sizes are integer bytes, timestamps UTC RFC 3339.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const SCHEMA_VERSION: u32 = 1;
/// Rows kept per section; the rest is counted in `total_rows` and `truncated` is set.
pub const ROW_CAP: usize = 100;
pub const DEFAULT_TOP_N: u32 = 25;
pub const MAX_TOP_N: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SectionId {
    Drives,
    Top,
    Cleanup,
    Duplicates,
    Stale,
    Tiering,
    Plans,
    Forecast,
}

impl SectionId {
    pub const ALL: [SectionId; 8] = [
        Self::Drives,
        Self::Top,
        Self::Cleanup,
        Self::Duplicates,
        Self::Stale,
        Self::Tiering,
        Self::Plans,
        Self::Forecast,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Drives => "drives",
            Self::Top => "top",
            Self::Cleanup => "cleanup",
            Self::Duplicates => "duplicates",
            Self::Stale => "stale",
            Self::Tiering => "tiering",
            Self::Plans => "plans",
            Self::Forecast => "forecast",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Drives => "Drive summary",
            Self::Top => "Largest folders and files",
            Self::Cleanup => "Cleanup candidates",
            Self::Duplicates => "Duplicate files",
            Self::Stale => "Stale data",
            Self::Tiering => "Tiering suggestions",
            Self::Plans => "Cleanup plans",
            Self::Forecast => "Free-space forecast",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Format {
    Html,
    Csv,
    Json,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Csv => "zip",
            Self::Json => "json",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Html => "text/html; charset=utf-8",
            Self::Csv => "application/zip",
            Self::Json => "application/json",
        }
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct ReportRequest {
    pub format: Format,
    /// Section keys; all sections when omitted.
    pub sections: Option<Vec<SectionId>>,
    /// Only this drive letter (`C`).
    pub drive: Option<String>,
    /// Rows in each Top list (1 to 100, default 25).
    pub top_n: Option<u32>,
    /// `save` only: the folder to write into.
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SectionInfo {
    pub id: SectionId,
    pub title: String,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SavedReport {
    pub path: String,
    pub file_name: String,
    #[ts(type = "number")]
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    NotAvailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section<T> {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Rows before the cap, when the section was cut.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_rows: Option<u64>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> Section<T> {
    pub fn ok(data: T) -> Self {
        Self { status: Status::Ok, reason: None, total_rows: None, truncated: false, data: Some(data) }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            status: Status::NotAvailable,
            reason: Some(reason.into()),
            total_rows: None,
            truncated: false,
            data: None,
        }
    }

    pub fn cut(mut self, total: usize, kept: usize) -> Self {
        if total > kept {
            self.total_rows = Some(total as u64);
            self.truncated = true;
        }
        self
    }

    pub fn is_ok(&self) -> bool {
        self.status == Status::Ok
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DriveRow {
    pub drive: String,
    pub label: String,
    pub filesystem: String,
    pub media: String,
    pub capacity: u64,
    pub free: u64,
    pub used: u64,
    pub used_percent: f64,
    pub health: String,
    pub low_space: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopEntry {
    pub path: String,
    pub allocated: u64,
    pub logical: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopRoot {
    pub root: String,
    pub snapshot_id: i64,
    pub scanned_at: Option<String>,
    pub folders: Vec<TopEntry>,
    pub files: Vec<TopEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavingRow {
    pub root: String,
    pub risk: String,
    pub kind: String,
    pub allocated: u64,
    pub logical: u64,
    pub matches: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CandidateRow {
    pub root: String,
    pub rule_id: String,
    pub title: String,
    pub risk: String,
    pub kind: String,
    pub path: String,
    pub allocated: Option<u64>,
    pub logical: Option<u64>,
    /// Manual steps, as text. diskdash never runs them.
    pub instructions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupSection {
    pub savings: Vec<SavingRow>,
    pub candidates: Vec<CandidateRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DupCopy {
    pub path: String,
    pub suggested: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DupRow {
    pub size: u64,
    pub wasted: u64,
    pub copies: Vec<DupCopy>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicatesSection {
    pub total_groups: u64,
    pub total_wasted: u64,
    pub groups: Vec<DupRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StaleRow {
    pub path: String,
    pub allocated: u64,
    pub last_used: String,
    pub age_days: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StaleRoot {
    pub root: String,
    pub months: u32,
    /// `atime` or `mtime`.
    pub time_basis: String,
    pub items: Vec<StaleRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StaleSection {
    pub roots: Vec<StaleRoot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierDriveRow {
    pub drive: String,
    pub capacity: u64,
    pub free: u64,
    pub projected_free: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierRow {
    pub path: String,
    pub direction: String,
    pub kind: String,
    pub allocated: u64,
    pub from_drive: String,
    pub to_drive: Option<String>,
    pub destination: Option<String>,
    pub reason: String,
    pub instructions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TieringSection {
    pub as_of: Option<String>,
    pub drives: Vec<TierDriveRow>,
    pub suggestions: Vec<TierRow>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanItemRow {
    pub path: String,
    pub title: String,
    pub risk: String,
    pub allocated: Option<u64>,
    pub status: String,
    pub destination: Option<String>,
    pub instructions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanRow {
    pub name: String,
    pub created_at: String,
    pub freed_total: i64,
    pub items: Vec<PlanItemRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlansSection {
    pub plans: Vec<PlanRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForecastRow {
    pub drive: String,
    pub capacity: u64,
    /// `not-enough-history`, `not-filling` or `filling`.
    pub state: String,
    pub days_to_low: Option<f64>,
    pub days_to_full: Option<f64>,
    pub bytes_per_day: Option<f64>,
    /// `high`, `low` or `none`.
    pub confidence: String,
    /// `[unix seconds, free bytes]`, thinned for the chart.
    pub samples: Vec<[i64; 2]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForecastSection {
    pub volumes: Vec<ForecastRow>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Sections {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drives: Option<Section<Vec<DriveRow>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<Section<Vec<TopRoot>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<Section<CleanupSection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicates: Option<Section<DuplicatesSection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<Section<StaleSection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiering: Option<Section<TieringSection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plans: Option<Section<PlansSection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast: Option<Section<ForecastSection>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scope {
    pub drive: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub generated_at: String,
    pub app_version: &'static str,
    pub scope: Scope,
    pub top_n: u32,
    pub sections: Sections,
}
