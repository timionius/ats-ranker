use anyhow::Result;

pub mod greenhouse;

#[derive(Debug)]
pub struct RawJobPosting {
    pub external_id: i64,
    pub internal_job_id: Option<i64>,
    pub title: String,
    pub updated_at: chrono::NaiveDateTime,
    pub requisition_id: Option<String>,
    pub location: serde_json::Value,
    pub absolute_url: String,
    pub language: Option<String>,
    pub metadata: serde_json::Value,
    pub content: String,
    pub departments: serde_json::Value,
    pub offices: serde_json::Value,
}

pub trait AtsProvider {
    fn fetch_jobs(&self, board: &str) -> Result<Vec<RawJobPosting>>;
}