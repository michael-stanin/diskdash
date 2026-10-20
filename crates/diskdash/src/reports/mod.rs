//! Report export: HTML, CSV (zipped) and JSON.
//!
//! Reports are built from stored data only and are either returned as a browser
//! download or written, as a new file, into a folder the user typed in. They hold
//! text for a person to read; nothing in them is executable and diskdash never
//! runs the instructions they contain.

pub mod assemble;
pub mod charts;
pub mod csv_zip;
pub mod html;
pub mod model;

pub use assemble::ReportService;
pub use model::*;

/// A report larger than this is refused rather than sent (rows are capped long before).
pub const MAX_REPORT_BYTES: usize = 5 * 1024 * 1024;

/// `diskdash-report-20261020-095701` from the report's timestamp.
pub fn file_stem(r: &Report) -> String {
    let digits: String = r.generated_at.chars().filter(char::is_ascii_digit).collect();
    if digits.len() >= 14 {
        format!("diskdash-report-{}-{}", &digits[..8], &digits[8..14])
    } else {
        "diskdash-report".to_string()
    }
}

pub fn render(r: &Report, format: Format) -> Result<Vec<u8>, String> {
    let bytes = match format {
        Format::Json => serde_json::to_vec_pretty(r).map_err(|e| e.to_string())?,
        Format::Csv => csv_zip::to_zip(r)?,
        Format::Html => html::render(r)?,
    };
    if bytes.len() > MAX_REPORT_BYTES {
        return Err(format!("the report is {} MB, over the {} MB limit", bytes.len() >> 20, MAX_REPORT_BYTES >> 20));
    }
    Ok(bytes)
}
