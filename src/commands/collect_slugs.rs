use anyhow::{Context, Result};
use duckdb::{Connection, params};
use regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn run(conn: &Connection, file: &Path) -> Result<()> {
    let re = Regex::new(r"boards\.greenhouse\.io/([^/]+)/").unwrap();

    let f = File::open(file).with_context(|| format!("failed to open {file:?}"))?;
    let reader = BufReader::new(f);

    let mut found = 0;
    let mut inserted = 0;
    let mut skipped_percent_encoded = 0;

    for (line_no, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("line {line_no}: read error: {e}");
                continue;
            }
        };

        let parsed: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // matches Python's silent `except: continue`
        };

        let url = match parsed.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => continue,
        };

        let Some(caps) = re.captures(url) else { continue };
        let board = caps[1].replace("%20", "").trim().to_string();

        if board.contains('%') {
            skipped_percent_encoded += 1;
            eprintln!("Error extracting slug from: {url}");
            continue;
        }

        found += 1;

        let result = conn.execute(
            "INSERT INTO boards (board) VALUES (?) ON CONFLICT (board) DO NOTHING",
            params![board],
        )?;

        if result > 0 {
            inserted += 1;
        }
    }

    println!(
        "Scanned {file:?}: {found} board URLs matched, {inserted} new boards inserted, {skipped_percent_encoded} skipped (percent-encoded)."
    );
    Ok(())
}