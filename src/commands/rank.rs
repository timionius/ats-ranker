use crate::deepseek::DeepSeekClient;
use crate::model::AtsScoring;
use crate::utils::{calculate_cost, clean_html_content};
use anyhow::{Context, Result};
use duckdb::{Connection, params};

pub fn run(conn: &Connection, cv_id: i32) -> Result<()> {
    let (raw_text,): (String,) = conn.query_row(
        "SELECT raw_text FROM cvs WHERE id = ?",
        params![cv_id],
        |row| Ok((row.get(0)?,)),
    ).with_context(|| format!("no CV found with id {cv_id}"))?;

    dotenvy::dotenv().expect("Failed to load .env file");
        
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .context("set DEEPSEEK_API_KEY environment variable")?;

    let ds = DeepSeekClient::new(&api_key, &raw_text);

    let mut stmt = conn.prepare(
        "SELECT j.id, j.title, j.content,
                array_cosine_similarity(e.embedding, c.embedding) AS similarity
         FROM jobs j
         JOIN job_embeddings e ON e.job_id = j.id
         CROSS JOIN cvs c
         LEFT JOIN cv_rank r ON r.job_id = j.id AND r.cv_id = c.id
         WHERE c.id = ?
           AND j.is_active = true
           AND (r.job_id IS NULL OR r.scoring_error IS NOT NULL)
         ORDER BY similarity DESC
         LIMIT 500",
    )?;

    let rows: Vec<(i64, String, String, f64)> = stmt
        .query_map(params![cv_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<_, _>>()?;

    let total = rows.len();
    let mut cumulative_cost = 0.0;

    println!("Ranking {total} candidate jobs for cv_id = {cv_id}...");

    for (i, (job_id, title, raw_content, similarity)) in rows.into_iter().enumerate() {
        let content = clean_html_content(&raw_content);
        println!("\n[{}/{total}] ID: {job_id}  TITLE: {title}  (similarity: {similarity:.3})", i + 1);

        match ds.score(&content) {
            Ok((scoring, usage, elapsed)) => {
                let cost = calculate_cost(&usage);
                cumulative_cost += cost;
                write_success(conn, cv_id, job_id, &scoring, similarity)?;
                write_metrics(conn, cv_id, job_id, &usage, cost, elapsed)?;
                println!("{}", serde_json::to_string_pretty(&scoring)?);
            }
            Err(e) => {
                eprintln!("Failed to score job {job_id} ({title}): {e:?}");
                write_error(conn, cv_id, job_id, &e.to_string(), similarity)?;
            }
        }
    }

    println!("\nDone. {total} jobs processed. Total cost: ${cumulative_cost:.3}");
    Ok(())
}

fn write_success(conn: &Connection, cv_id: i32, job_id: i64, scoring: &AtsScoring, similarity: f64) -> Result<()> {
    conn.execute(
        "INSERT INTO cv_rank (
            cv_id, job_id, ats_score, recruiter_score, remote_type, b2b_friendly,
            strengths, red_flags, summary, scoring_model, scored_at, scoring_error, similarity
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, now(), NULL, ?)
         ON CONFLICT (cv_id, job_id) DO UPDATE SET
            ats_score = excluded.ats_score,
            recruiter_score = excluded.recruiter_score,
            remote_type = excluded.remote_type,
            b2b_friendly = excluded.b2b_friendly,
            strengths = excluded.strengths,
            red_flags = excluded.red_flags,
            summary = excluded.summary,
            scoring_model = excluded.scoring_model,
            scored_at = excluded.scored_at,
            scoring_error = NULL,
            similarity = excluded.similarity",
        params![
            cv_id, job_id, scoring.ats_score, scoring.recruiter_score, scoring.remote_type,
            scoring.b2b_friendly, serde_json::to_string(&scoring.strengths)?,
            serde_json::to_string(&scoring.red_flags)?, scoring.summary,
            "deepseek-v4-flash", similarity,
        ],
    )?;
    Ok(())
}

fn write_error(conn: &Connection, cv_id: i32, job_id: i64, error_msg: &str, similarity: f64) -> Result<()> {
    conn.execute(
        "INSERT INTO cv_rank (cv_id, job_id, scoring_error, scored_at, similarity)
         VALUES (?, ?, ?, now(), ?)
         ON CONFLICT (cv_id, job_id) DO UPDATE SET
            scoring_error = excluded.scoring_error,
            scored_at = excluded.scored_at,
            similarity = excluded.similarity",
        params![cv_id, job_id, error_msg, similarity],
    )?;
    Ok(())
}

fn write_metrics(conn: &Connection, cv_id: i32, job_id: i64, usage: &crate::model::Usage, cost: f64, latency: f64) -> Result<()> {
    conn.execute(
        "INSERT INTO scoring_metrics (
            cv_id, job_id, prompt_tokens, prompt_cache_hit_tokens, prompt_cache_miss_tokens,
            completion_tokens, cost_usd, latency_seconds, scored_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, now())",
        params![
            cv_id, job_id, usage.prompt_tokens, usage.prompt_cache_hit_tokens,
            usage.prompt_cache_miss_tokens, usage.completion_tokens, cost, latency,
        ],
    )?;
    Ok(())
}