use crate::providers::{AtsProvider, greenhouse::GreenhouseProvider};
use anyhow::Result;
use duckdb::{Connection, params};
use reqwest::blocking::Client;
use std::time::Duration;

pub fn run(conn: &Connection) -> Result<()> {
    let client = Client::builder()
        .pool_max_idle_per_host(0)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("failed to build reqwest client");
    let provider = GreenhouseProvider::new(client);

    let mut stmt = conn.prepare("SELECT board FROM boards ORDER BY board")?;
    let boards: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    let total = boards.len();
    let mut succeeded = 0;
    let mut failed = 0;

    println!("Refreshing {total} boards...");

    for (i, board) in boards.iter().enumerate() {
        match provider.fetch_jobs(board) {
            Ok(jobs) => {
                let mut inserted_or_updated = 0;
                for job in jobs {
                    let content_hash = crate::utils::hash_content(&job.content);

                    let existing_updated_at: Option<chrono::NaiveDateTime> = conn
                        .query_row(
                            "SELECT updated_at FROM jobs WHERE id = ?",
                            params![job.external_id],
                            |row| row.get(0),
                        )
                        .ok();

                    let should_write = match existing_updated_at {
                        Some(existing) => job.updated_at > existing,
                        None => true, // new job, always insert
                    };

                    if !should_write {
                        continue;
                    }

                    conn.execute(
                        "INSERT INTO jobs (
                            board, id, internal_job_id, title, updated_at, requisition_id,
                            location, absolute_url, language, metadata, content, departments,
                            offices, content_hash, is_active, closed_at
                         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, true, NULL)
                         ON CONFLICT (id) DO UPDATE SET
                            title = excluded.title,
                            updated_at = excluded.updated_at,
                            requisition_id = excluded.requisition_id,
                            location = excluded.location,
                            absolute_url = excluded.absolute_url,
                            language = excluded.language,
                            metadata = excluded.metadata,
                            content = excluded.content,
                            departments = excluded.departments,
                            offices = excluded.offices,
                            content_hash = excluded.content_hash,
                            is_active = true,
                            closed_at = NULL",
                        params![
                            board, job.external_id, job.internal_job_id, job.title,
                            job.updated_at, job.requisition_id, job.location.to_string(),
                            job.absolute_url, job.language, job.metadata.to_string(),
                            job.content, job.departments.to_string(), job.offices.to_string(),
                            content_hash,
                        ],
                    )?;
                    inserted_or_updated += 1;
                }

                conn.execute(
                    "UPDATE boards SET last_refreshed_at = now() WHERE board = ?",
                    params![board],
                )?;

                succeeded += 1;
                println!("[{}/{total}] {board}: ok ({inserted_or_updated} jobs written)", i + 1);
            }
            Err(e) => {
                failed += 1;
                eprintln!("[{}/{total}] {board}: FAILED — {e:?}", i + 1);
            }
        }

        std::thread::sleep(Duration::from_millis(150));
    }

    println!("\nDone. {succeeded} boards refreshed, {failed} failed.");
    Ok(())
}