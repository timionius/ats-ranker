use anyhow::{Context, Result};
use duckdb::Connection;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::time::Duration;

mod deepseek;
use deepseek::DeepSeekClient;

mod utils;
use utils::{PREWARM_TEMPLATE, ASSESSMENT_TEMPLATE, build_schema, calculate_cost, clean_html_content};

mod model;
use model::{AtsScoring, CompletionRequest, CompletionResponse, Usage};

const SERVER_URL: &str = "http:/192.168.1.79:8080";
const API_KEY: &str = "QQQ%123";

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


const MODEL_NAME: &str = "qwen2.5-1.5b-instruct-q4_k_m";

fn main() -> Result<()> {
    let resume = fs::read_to_string("resume.txt").context("failed to read resume.txt")?;
    // LOCAL VERSION
    // let ats = AtsClient::new(&resume);
    // println!("Warming up prompt cache with resume prefix...");
    // ats.warm_up()?;
    // println!("Cache warmed. Starting vacancy scoring.");
    dotenvy::dotenv().expect("Failed to load .env file");
        
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .context("set DEEPSEEK_API_KEY environment variable")?;
    let ds = DeepSeekClient::new(&api_key, &resume);

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

    // let rows = stmt.query_map([], |row| {
    //     Ok((
    //         row.get::<_, i64>(0)?,
    //         row.get::<_, String>(1)?,
    //         row.get::<_, String>(2)?,
    //         row.get::<_, String>(3)?,
    //     ))
    // })?;

    // for row in rows {
    //     let (id, title, _url, content) = row?;

    //     println!("\nID: {id}\nTITLE: {title}");
    //     let clean_content = clean_html_content(&content);
    //     match ats.score(&clean_content) {
    //         Ok(scoring) => {
    //             println!("{}", serde_json::to_string_pretty(&scoring)?);
    //             AtsClient::write_success(&conn, id, &scoring, MODEL_NAME)?;
    //         }
    //         Err(e) => {
    //             eprintln!("Failed to score job {id} ({title}): {e:?}");
    //             AtsClient::write_error(&conn, id, &e.to_string())?;
    //         }
    //     }
    // }

    let rows: Vec<(i64, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<_, _>>()?;

    let total = rows.len();
    let mut cumulative_cost = 0.0;
    let mut cumulative_time = 0.0;
    let batch_start = std::time::Instant::now();

    println!("Scoring {total} vacancies via DeepSeek...");

    for (i, (id, title, _url, content)) in rows.into_iter().enumerate() {
        println!("\n[{}/{total}] ID: {id}  TITLE: {title}", i + 1);
        let clean_content = clean_html_content(&content);
        match ds.score(&clean_content) {
            Ok((scoring, usage, elapsed)) => {
                let cost = calculate_cost(&usage);
                cumulative_cost += cost;
                cumulative_time += elapsed;

                println!("{}", serde_json::to_string_pretty(&scoring)?);
                AtsClient::write_success(&conn, id, &scoring, MODEL_NAME)?;
                write_metrics(&conn, id, &usage, cost, elapsed)?;

                if (i + 1) % 25 == 0 {
                    let wall_elapsed = batch_start.elapsed().as_secs_f64();
                    let avg_per_job = wall_elapsed / (i + 1) as f64;
                    let remaining = (total - (i + 1)) as f64 * avg_per_job;
                    println!(
                        "\n--- Progress: {}/{total} | cumulative cost: ${:.3} | avg {:.1}s/job | est. remaining: {:.0}min ---",
                        i + 1, cumulative_cost, avg_per_job, remaining / 60.0
                    );
                }
            }
            Err(e) => {
                eprintln!("Failed to score job {id} ({title}): {e:?}");
                AtsClient::write_error(&conn, id, &e.to_string())?;
            }
        }
    }
    Ok(())
}

fn write_metrics(conn: &Connection, job_id: i64, usage: &Usage, cost: f64, latency: f64) -> Result<()> {
    conn.execute(
        "INSERT INTO scoring_metrics (
            job_id, prompt_tokens, prompt_cache_hit_tokens, prompt_cache_miss_tokens,
            completion_tokens, cost_usd, latency_seconds, scored_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, now())",
        duckdb::params![
            job_id,
            usage.prompt_tokens,
            usage.prompt_cache_hit_tokens,
            usage.prompt_cache_miss_tokens,
            usage.completion_tokens,
            cost,
            latency,
        ],
    )?;
    Ok(())
}
