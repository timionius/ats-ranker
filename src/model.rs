use serde::{Deserialize, Serialize};
use serde_json::{Value};

// local AtsScoring model
#[derive(Serialize)]
pub struct CompletionRequest {
    pub(crate) prompt: String,
    pub(crate) n_predict: u32,
    pub(crate) temperature: f32,
    pub(crate) repeat_penalty: f32,
    pub(crate) cache_prompt: bool,
    pub(crate) json_schema: Value,
}

#[derive(Deserialize, Debug)]
pub struct CompletionResponse {
    pub(crate) content: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct RedFlag {
    pub(crate) severity: String,
    pub(crate) reason: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct AtsScoring {
    pub(crate) ats_score: u32,
    pub(crate) recruiter_score: u32,
    pub(crate) remote_type: String,
    pub(crate) b2b_friendly: bool,
    pub(crate) strengths: Vec<String>,
    pub(crate) red_flags: Vec<RedFlag>,
    pub(crate) summary: String,
}

// Deepseek model
#[derive(Serialize)]
pub struct ChatMessage {
    pub(crate) role: String,
    pub(crate) content: String,
}

// Fallback mode: DeepSeek's json_schema support returned 400 Bad Request,
// so we're using the more universally-supported json_object mode instead.
// The schema shape itself now lives in the prompt text (assessment.txt),
// not enforced by the API — see note in DeepSeekClient::score().
#[derive(Serialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub(crate) format_type: String,
}

impl ResponseFormat {
    pub fn json_object() -> Self {
        Self {
            format_type: "json_object".to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub(crate) mode: String,
}

#[derive(Serialize)]
pub struct ChatRequest {
    pub(crate) model: String,
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) temperature: f32,
    pub(crate) max_tokens: u32,
    pub(crate) response_format: ResponseFormat,
    pub(crate) thinking: ThinkingConfig,
}

#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    pub(crate) choices: Vec<ChatChoice>,
    pub(crate) usage: Usage,
}

#[derive(Deserialize, Debug)]
pub struct ChatChoice {
    pub(crate) message: ChatResponseMessage,
    #[serde(default)]
    pub(crate) finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct ChatResponseMessage {
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) reasoning_content: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Usage {
    pub(crate) prompt_tokens: u32,
    pub(crate) completion_tokens: u32,
    pub(crate) total_tokens: u32,
    #[serde(default)]
    pub(crate) prompt_cache_hit_tokens: u32,
    #[serde(default)]
    pub(crate) prompt_cache_miss_tokens: u32,
}