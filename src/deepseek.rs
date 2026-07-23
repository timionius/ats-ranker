use anyhow::{Context, Result};
use crate::model::{AtsScoring, ChatMessage, ChatResponse, ChatRequest, ResponseFormat, Usage};
use crate::utils::{PREWARM_TEMPLATE, ASSESSMENT_TEMPLATE, build_schema, calculate_cost};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::time::Duration;


const DEEPSEEK_URL: &str = "https://api.deepseek.com/chat/completions";
const MODEL_NAME: &str = "deepseek-v4-flash";

pub struct DeepSeekClient {
    client: Client,
    api_key: String,
    fixed_prefix: String,
    schema: Value,
}

impl DeepSeekClient {
    pub fn new(api_key: &str, resume: &str) -> Self {
        let fixed_prefix = PREWARM_TEMPLATE.replace("{{resume}}", resume.trim());
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            api_key: api_key.to_string(),
            fixed_prefix,
            schema: build_schema(),
        }
    }

    pub fn score(&self, job_description: &str) -> Result<(AtsScoring, Usage, f64)> {
        for attempt in 1..=2 {
            match self.try_score(job_description) {
                Ok(result) => return Ok(result),
                Err(e) if attempt == 1 => {
                    eprintln!("  attempt 1 failed ({e}), retrying once...");
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }
    
    fn try_score(&self, job_description: &str) -> Result<(AtsScoring, Usage, f64)> {
        let suffix = ASSESSMENT_TEMPLATE.replace("{{job}}", job_description.trim());
        let user_content = format!("{}\n{}", self.fixed_prefix, suffix);

        let request = ChatRequest {
            model: MODEL_NAME.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: user_content,
            }],
            temperature: 0.25,
            max_tokens: 2000,
            response_format: ResponseFormat::json_object(),
        };
        let start = std::time::Instant::now();

        let raw_response = self
            .client
            .post(DEEPSEEK_URL)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .context("DeepSeek request failed")?;
            
        let status = raw_response.status();
        let body_text = raw_response.text().context("failed to read response body")?;
            
        if !status.is_success() {
            anyhow::bail!("DeepSeek returned {status}: {body_text}");
        }

        let response: ChatResponse = serde_json::from_str(&body_text)
            .context("failed to parse DeepSeek response envelope")?;

        let elapsed = start.elapsed().as_secs_f64();

        let content = &response.choices.get(0)
            .context("no choices in response")?
            .message.content;

        if content.trim().is_empty() {
            anyhow::bail!("model returned empty content (possible content filter or generation failure)");
        }

        let scoring = Self::parse_scoring(content)
            .with_context(|| format!("raw content was: {content}"))?;

        let usage = response.usage.clone();
        let cost = calculate_cost(&usage);

        eprintln!(
            "  tokens: {}in ({}hit/{}miss) + {}out | cost: ${:.5} | latency: {:.1}s",
            usage.prompt_tokens, usage.prompt_cache_hit_tokens, usage.prompt_cache_miss_tokens,
            usage.completion_tokens, cost, elapsed
        );

        Ok((scoring, usage, elapsed))
    }

    fn parse_scoring(content: &str) -> Result<AtsScoring> {
        let mut raw: Value = serde_json::from_str(content)
            .context("response is not valid JSON at all")?;

        if let Some(red_flags) = raw.get_mut("red_flags").and_then(|v| v.as_array_mut()) {
            for item in red_flags.iter_mut() {
                if item.is_string() {
                    let reason = item.as_str().unwrap().to_string();
                    *item = json!({"severity": "unknown", "reason": reason});
                }
            }
        }

        if let Some(b2b) = raw.get_mut("b2b_friendly") {
            if !b2b.is_boolean() {
                *b2b = Value::Null;
            }
        }

        if let Some(remote) = raw.get_mut("remote_type").and_then(|v| v.as_str()) {
            let normalized = Self::normalize_remote_type(remote);
            raw["remote_type"] = json!(normalized);
        }

        serde_json::from_value(raw).context("failed to parse coerced JSON into AtsScoring")
    }

    fn normalize_remote_type(value: &str) -> String {
        let v = value.to_lowercase().replace('_', " ").replace('-', " ");
        if v.contains("remote first") || v == "fully remote" {
            "remote_first".to_string()
        } else if v.contains("remote") {
            "remote_friendly".to_string()
        } else if v.contains("hybrid") {
            "hybrid".to_string()
        } else if v.contains("onsite") || v.contains("on site") || v.contains("office") {
            "office".to_string()
        } else {
            "unknown".to_string()
        }
    }
}
