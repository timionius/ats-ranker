use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;
mod providers;
mod deepseek;
mod embeddings;
mod model;
mod db;
mod utils;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extract Greenhouse board slugs from a Common Crawl index file
    CollectSlugs { file: PathBuf },
    /// Fetch/update job postings for all known boards
    JdRefresh,
    /// Compute embeddings
    Vectorize {
        #[command(subcommand)]
        target: VectorizeTarget,
    },
    /// Rank all active jobs against a CV
    Rank { cv_id: i32 },
}

#[derive(Subcommand)]
enum VectorizeTarget {
    Jd,
    Cv { file: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let conn = db::connect()?;

    match cli.command {
        Command::CollectSlugs { file } => commands::collect_slugs::run(&conn, &file),
        Command::JdRefresh => commands::jd_refresh::run(&conn),
        Command::Vectorize { target } => match target {
            VectorizeTarget::Jd => commands::vectorize::run_jd(&conn),
            VectorizeTarget::Cv { file } => commands::vectorize::run_cv(&conn, &file),
        },
        Command::Rank { cv_id } => commands::rank::run(&conn, cv_id),
    }
}

#[test]
fn debug_parse_saved_file() {
    let bytes = std::fs::read("/tmp/10beauty_failed.json").unwrap();

    // Try parsing into a generic Value first — no custom struct involved
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(_) => println!("Generic Value parse: SUCCESS"),
        Err(e) => println!("Generic Value parse: FAILED — {e}"),
    }
}