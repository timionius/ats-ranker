use anyhow::{Context, Result};
use duckdb::Connection;

const DB_PATH: &str = "../greenhouse.duckdb";

pub fn connect() -> Result<Connection> {
    Connection::open(DB_PATH).context("failed to open DuckDB database")
}