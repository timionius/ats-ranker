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