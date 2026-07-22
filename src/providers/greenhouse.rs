use super::{AtsProvider, RawJobPosting};
use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset};
use reqwest::blocking::Client;
use serde::Deserialize;

pub struct GreenhouseProvider {
    client: Client,
}

impl GreenhouseProvider {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[derive(Deserialize)]
struct GreenhouseJobsResponse {
    jobs: Vec<GreenhouseJob>,
}

#[derive(Deserialize)]
struct GreenhouseJob {
    id: i64,
    internal_job_id: Option<i64>,
    title: Option<String>,
    updated_at: Option<DateTime<FixedOffset>>,
    requisition_id: Option<String>,
    location: Option<serde_json::Value>,
    absolute_url: Option<String>,
    language: Option<String>,
    metadata: Option<serde_json::Value>,
    content: Option<String>,
    departments: Option<serde_json::Value>,
    offices: Option<serde_json::Value>,
}

impl AtsProvider for GreenhouseProvider {
    fn fetch_jobs(&self, board: &str) -> Result<Vec<RawJobPosting>> {
        let url = format!("https://boards-api.greenhouse.io/v1/boards/{board}/jobs?content=true");

        let response = self
            .client
            .get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .with_context(|| format!("request failed for board '{board}'"))?;

        if !response.status().is_success() {
            anyhow::bail!("board '{board}' returned HTTP {}", response.status());
        }

        // Read raw bytes and parse directly as JSON — mirrors Python's `r.json()`,
        // which decodes bytes straight into JSON without reqwest's `.text()`
        // charset-detection layer in between.
        let raw_bytes = response
            .bytes()
            .with_context(|| format!("failed to read response bytes for board '{board}'"))?;

        let parsed: GreenhouseJobsResponse = serde_json::from_slice(&raw_bytes).map_err(|e| {
            std::fs::write(format!("/tmp/{board}_failed.json"), &raw_bytes).ok();
            anyhow::anyhow!(
                "failed to parse jobs JSON for board '{board}': {e}. Raw bytes saved to /tmp/{board}_failed.json"
            )
        })?;

        Ok(parsed
            .jobs
            .into_iter()
            .filter_map(|j| {
                Some(RawJobPosting {
                    external_id: j.id,
                    internal_job_id: j.internal_job_id,
                    title: j.title?,
                    updated_at: j.updated_at?.naive_local(),
                    requisition_id: j.requisition_id,
                    location: j.location.unwrap_or(serde_json::Value::Null),
                    absolute_url: j.absolute_url?,
                    language: j.language,
                    metadata: j.metadata.unwrap_or(serde_json::Value::Null),
                    content: j.content?,
                    departments: j.departments.unwrap_or(serde_json::Value::Null),
                    offices: j.offices.unwrap_or(serde_json::Value::Null),
                })
            })
            .collect())
    }
}