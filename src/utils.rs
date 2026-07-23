use serde_json::{json, Value};
use crate::model::Usage;
use sha2::{Digest, Sha256};

pub const PREWARM_TEMPLATE: &str = include_str!("../prompts/prewarm.txt");
pub const ASSESSMENT_TEMPLATE: &str = include_str!("../prompts/assessment.txt");

// DeepSeek v4-flash pricing, per 1M tokens (verify against
// https://api-docs.deepseek.com/quick_start/pricing before a large batch run)
pub const PRICE_INPUT_CACHE_MISS: f64 = 0.14;
pub const PRICE_INPUT_CACHE_HIT: f64 = 0.0028;
pub const PRICE_OUTPUT: f64 = 0.28;

pub fn build_schema() -> Value {
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

pub fn calculate_cost(usage: &Usage) -> f64 {
    let hit_cost = (usage.prompt_cache_hit_tokens as f64 / 1_000_000.0) * PRICE_INPUT_CACHE_HIT;
    let miss_cost = (usage.prompt_cache_miss_tokens as f64 / 1_000_000.0) * PRICE_INPUT_CACHE_MISS;
    let output_cost = (usage.completion_tokens as f64 / 1_000_000.0) * PRICE_OUTPUT;
    hit_cost + miss_cost + output_cost
}

pub fn clean_html_content(input: &str) -> String {
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

pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn embedding_to_sql_literal(embedding: &[f32]) -> String {
    let values: Vec<String> = embedding.iter().map(|v| v.to_string()).collect();
    format!("[{}]", values.join(","))
}

pub fn chunk_text(text: &str, max_words: usize, overlap: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return vec![String::new()];
    }
    if words.len() <= max_words {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < words.len() {
        let end = (start + max_words).min(words.len());
        chunks.push(words[start..end].join(" "));
        if end == words.len() {
            break;
        }
        start += max_words - overlap;
    }
    chunks
}

pub fn mean_pool_and_normalize(vectors: &[Vec<f32>]) -> Vec<f32> {
    let dim = vectors[0].len();
    let mut mean = vec![0.0f32; dim];
    for v in vectors {
        for (i, x) in v.iter().enumerate() {
            mean[i] += x;
        }
    }
    let n = vectors.len() as f32;
    for x in mean.iter_mut() {
        *x /= n;
    }
    let norm: f32 = mean.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in mean.iter_mut() {
            *x /= norm;
        }
    }
    mean
}

pub fn vectorize_text(embedder: &mut fastembed::TextEmbedding, text: &str) -> anyhow::Result<Vec<f32>> {
    let chunks = chunk_text(text, 350, 50);
    let chunk_embeddings = crate::embeddings::get_embeddings_batch(embedder, chunks)?;
    Ok(mean_pool_and_normalize(&chunk_embeddings))
}