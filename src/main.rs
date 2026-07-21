use anyhow::{Context, Result};
use duckdb::Connection;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::time::Duration;

const PREWARM_TEMPLATE: &str = include_str!("../prompts/prewarm.txt");
const ASSESSMENT_TEMPLATE: &str = include_str!("../prompts/assessment.txt");

const SERVER_URL: &str = "http:/192.168.1.79:8080";
const API_KEY: &str = "QQQ%123";

#[derive(Serialize)]
struct CompletionRequest {
    prompt: String,
    n_predict: u32,
    temperature: f32,
    repeat_penalty: f32,
    cache_prompt: bool,
    json_schema: Value,
}

#[derive(Deserialize, Debug)]
struct CompletionResponse {
    content: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct RedFlag {
    severity: String,
    reason: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct AtsScoring {
    ats_score: u32,
    recruiter_score: u32,
    remote_type: String,
    b2b_friendly: bool,
    strengths: Vec<String>,
    red_flags: Vec<RedFlag>,
    summary: String,
}

struct AtsClient {
    client: Client,
    fixed_prefix: String,
    schema: Value,
}

impl AtsClient {
    fn new(resume: &str) -> Self {
        let fixed_prefix = PREWARM_TEMPLATE.replace("{{resume}}", resume.trim());

        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            fixed_prefix,
            schema: build_schema(),
        }
    }

    /// Populates the server's slot KV-cache with the resume prefix before real traffic starts.
    fn warm_up(&self) -> Result<()> {
        let req = json!({
            "prompt": self.fixed_prefix,
            "n_predict": 1,
            "cache_prompt": true
        });

        self.client
            .post(format!("{SERVER_URL}/completion"))
            .bearer_auth(API_KEY)
            .json(&req)
            .send()
            .context("warm-up request failed")?
            .error_for_status()
            .context("warm-up request returned error status")?;

        Ok(())
    }

    fn score(&self, job_description: &str) -> Result<AtsScoring> {
        let suffix = ASSESSMENT_TEMPLATE.replace("{{job}}", job_description.trim());
        let full_prompt = format!("{}\n{}", self.fixed_prefix, suffix);

        let request = CompletionRequest {
            prompt: full_prompt,
            n_predict: 1000,
            temperature: 0.25,
            repeat_penalty: 1.15,
            cache_prompt: true,
            json_schema: self.schema.clone(),
        };

        let response: CompletionResponse = self
            .client
            .post(format!("{SERVER_URL}/completion"))
            .bearer_auth(API_KEY)
            .json(&request)
            .send()
            .context("scoring request failed")?
            .error_for_status()
            .context("scoring request returned error status")?
            .json()
            .context("failed to parse server response envelope")?;

        serde_json::from_str(&response.content).context("failed to parse model JSON content")
    }

    fn write_success(
        conn: &Connection,
        job_id: i64,
        scoring: &AtsScoring,
        model_name: &str,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO cv_rank (
                job_id, ats_score, recruiter_score, remote_type, b2b_friendly,
                strengths, red_flags, summary, scoring_model, scored_at, scoring_error
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, now(), NULL)
             ON CONFLICT (job_id) DO UPDATE SET
                ats_score = excluded.ats_score,
                recruiter_score = excluded.recruiter_score,
                remote_type = excluded.remote_type,
                b2b_friendly = excluded.b2b_friendly,
                strengths = excluded.strengths,
                red_flags = excluded.red_flags,
                summary = excluded.summary,
                scoring_model = excluded.scoring_model,
                scored_at = excluded.scored_at,
                scoring_error = NULL",
            duckdb::params![
                job_id,
                scoring.ats_score,
                scoring.recruiter_score,
                scoring.remote_type,
                scoring.b2b_friendly,
                serde_json::to_string(&scoring.strengths)?,
                serde_json::to_string(&scoring.red_flags)?,
                scoring.summary,
                model_name,
            ],
        )?;
        Ok(())
    }

    fn write_error(conn: &Connection, job_id: i64, error_msg: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO cv_rank (job_id, scoring_error, scored_at)
             VALUES (?, ?, now())
             ON CONFLICT (job_id) DO UPDATE SET
                scoring_error = excluded.scoring_error,
                scored_at = excluded.scored_at",
            duckdb::params![job_id, error_msg],
        )?;
        Ok(())
    }
}

fn build_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "strengths": {"type": "array", "items": {"type": "string", "maxLength": 200}, "maxItems": 6},
            "red_flags": {
                "type": "array",
                "minItems": 1,
                "maxItems": 6,
                "items": {
                    "type": "object",
                    "properties": {
                        "severity": {"type": "string", "enum": ["high", "medium", "low"]},
                        "reason": {"type": "string", "maxLength": 200}
                    },
                    "required": ["severity", "reason"],
                    "additionalProperties": false
                }
            },
            "summary": {"type": "string", "minLength": 20, "maxLength": 400},
            "ats_score": {"type": "integer", "minimum": 0, "maximum": 100},
            "recruiter_score": {"type": "integer", "minimum": 0, "maximum": 100},
            "remote_type": {"type": "string", "enum": ["remote_first", "remote_friendly", "hybrid", "office", "unknown"]},
            "b2b_friendly": {"type": "boolean"}
        },
        "required": ["strengths", "red_flags", "summary", "ats_score", "recruiter_score", "remote_type", "b2b_friendly"],
        "additionalProperties": false
    })
}

const MODEL_NAME: &str = "qwen2.5-1.5b-instruct-q4_k_m";

fn main() -> Result<()> {
    let resume = fs::read_to_string("resume.txt").context("failed to read resume.txt")?;
    let ats = AtsClient::new(&resume);

    println!("Warming up prompt cache with resume prefix...");
    ats.warm_up()?;
    println!("Cache warmed. Starting vacancy scoring.");

    let conn = Connection::open("../greenhouse.duckdb")?;
    let mut stmt = conn.prepare(
        "
        SELECT j.id, j.title, j.absolute_url, j.content
        FROM jobs j
        LEFT JOIN cv_rank r ON r.job_id = j.id
        WHERE r.job_id IS NULL OR r.scoring_error IS NOT NULL
            AND (lower(j.title) like '%android%' or lower(j.title) like '%mobile%')
        ORDER BY id DESC
        LIMIT 5
        ",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    for row in rows {
        let (id, title, _url, content) = row?;

        println!("\nID: {id}\nTITLE: {title}");
        let clean_content = clean_html_content(&content);
        match ats.score(&clean_content) {
            Ok(scoring) => {
                println!("{}", serde_json::to_string_pretty(&scoring)?);
                AtsClient::write_success(&conn, id, &scoring, MODEL_NAME)?;
            }
            Err(e) => {
                eprintln!("Failed to score job {id} ({title}): {e:?}");
                AtsClient::write_error(&conn, id, &e.to_string())?;
            }
        }
    }

    Ok(())
}

fn clean_html_content(input: &str) -> String {
    let unescaped = input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");

    let mut result = String::new();
    let mut in_tag = false;
    for c in unescaped.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }

    result.split_whitespace().collect::<Vec<_>>().join(" ")
}