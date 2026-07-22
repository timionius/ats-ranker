use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

const EMBED_URL: &str = "http://192.168.1.87:8081/embedding";
const EMBED_API_KEY: &str = "QQQ%123";

#[derive(Serialize)]
struct EmbedRequest {
    content: String,
}

#[derive(Deserialize, Debug)]
struct EmbedResponseItem {
    embedding: Vec<Vec<f32>>,
}

pub fn get_embedding(client: &Client, text: &str) -> Result<Vec<f32>> {
    let resp: Vec<EmbedResponseItem> = client
        .post(EMBED_URL)
        .bearer_auth(EMBED_API_KEY)
        .json(&EmbedRequest { content: text.to_string() })
        .send()
        .context("embedding request failed")?
        .json()
        .context("failed to parse embedding response")?;

    resp.into_iter()
        .next()
        .and_then(|item| item.embedding.into_iter().next())
        .context("no embedding vector returned")
}