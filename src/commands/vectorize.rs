use crate::embeddings::{build_embedder, get_embedding, get_embeddings_batch};
use crate::utils::{clean_html_content, hash_content, embedding_to_sql_literal};
use anyhow::{Context, Result};
use duckdb::{Connection, params};
use std::path::Path;

const BATCH_SIZE: usize = 32;

pub fn run_jd(conn: &Connection) -> Result<()> {
    let mut embedder = build_embedder()?;

    let mut stmt = conn.prepare(
        "SELECT j.id, j.content, j.content_hash
         FROM jobs j
         LEFT JOIN job_embeddings e ON e.job_id = j.id
         WHERE j.is_active = true
           AND (e.job_id IS NULL OR e.content_hash IS DISTINCT FROM j.content_hash)",
    )?;

    let rows: Vec<(i64, String, Option<String>)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;

    let total = rows.len();
    println!("Embedding {total} jobs in batches of {BATCH_SIZE}...");

    let mut succeeded = 0;
    let mut failed = 0;

    for (batch_idx, chunk) in rows.chunks(BATCH_SIZE).enumerate() {
        let job_ids: Vec<i64> = chunk.iter().map(|(id, _, _)| *id).collect();
        let hashes: Vec<String> = chunk
            .iter()
            .map(|(_, raw, hash)| hash.clone().unwrap_or_else(|| hash_content(raw)))
            .collect();
        let cleaned_texts: Vec<String> = chunk
            .iter()
            .map(|(_, raw, _)| clean_html_content(raw))
            .collect();

        match get_embeddings_batch(&mut embedder, cleaned_texts) {
            Ok(embeddings) => {
                for ((job_id, hash), embedding) in job_ids.iter().zip(hashes.iter()).zip(embeddings.iter()) {
                    let literal = embedding_to_sql_literal(embedding);
                    let query = format!(
                        "INSERT INTO job_embeddings (job_id, embedding, content_hash, embedded_at)
                         VALUES (?, {literal}::FLOAT[384], ?, now())
                         ON CONFLICT (job_id) DO UPDATE SET
                            embedding = excluded.embedding,
                            content_hash = excluded.content_hash,
                            embedded_at = excluded.embedded_at"
                    );
                    match conn.execute(&query, params![job_id, hash]) {
                        Ok(_) => succeeded += 1,
                        Err(e) => {
                            failed += 1;
                            eprintln!("job {job_id}: DB write failed — {e:?}");
                        }
                    }
                }
            }
            Err(e) => {
                failed += chunk.len();
                eprintln!("batch {batch_idx}: embedding failed for {} jobs — {e:?}", chunk.len());
            }
        }

        let processed = (batch_idx + 1) * BATCH_SIZE;
        println!(
            "  ~{}/{total} processed ({succeeded} ok, {failed} failed)",
            processed.min(total)
        );
    }

    println!("\nDone. {succeeded} embedded, {failed} failed.");
    Ok(())
}

pub fn run_cv(conn: &Connection, file: &Path) -> Result<()> {
    let mut embedder = build_embedder()?;

    let raw_text = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {file:?}"))?;

    let label = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("cv")
        .to_string();

    let embedding = get_embedding(&mut embedder, &raw_text)?;
    let literal = embedding_to_sql_literal(&embedding);

    let query = format!(
        "INSERT INTO cvs (label, raw_text, embedding, embedded_at, created_at)
         VALUES (?, ?, {literal}::FLOAT[384], now(), now())
         RETURNING id"
    );

    let new_id: i32 = conn.query_row(&query, params![label, raw_text], |row| row.get(0))?;

    println!("CV embedded and stored as cv_id = {new_id} (label: '{label}')");
    Ok(())
}