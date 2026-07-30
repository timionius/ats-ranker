use anyhow::{Context, Result};
use duckdb::Connection;

const DB_PATH: &str = "./ats_runker.duckdb";

pub fn connect() -> Result<Connection> {
    Connection::open(DB_PATH).context("failed to open DuckDB database")
}