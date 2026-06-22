//! A thin [`ModelClient`] over Gemini's OpenAI-compatible chat endpoint. Kept
//! deliberately small (HTTP + JSON, as DESIGN §6 calls for) and exercised only
//! in production — ranking logic is tested against the in-memory fake instead.

use std::cmp::Ordering;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::model::{JobText, MatchScore, ModelClient};

/// Gemini's OpenAI-compatible base URL.
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

/// Outbound request timeout, so an upstream stall can't hang a ranking call.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Cheap-model client. Holds separate models for the absolute (`match`) and
/// comparative (`pairwise`) passes, per DESIGN's config.
#[derive(Clone)]
pub struct GeminiClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    match_model: String,
    pair_model: String,
}

impl std::fmt::Debug for GeminiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the API key, so it can't leak via logs or error output.
        f.debug_struct("GeminiClient")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("match_model", &self.match_model)
            .field("pair_model", &self.pair_model)
            .finish_non_exhaustive()
    }
}

impl GeminiClient {
    /// Builds a client. `base_url` empty falls back to [`DEFAULT_BASE_URL`].
    #[must_use]
    pub fn new(api_key: String, base_url: String, match_model: String, pair_model: String) -> Self {
        let base_url = if base_url.trim().is_empty() {
            DEFAULT_BASE_URL.to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        Self {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client builds with a timeout"),
            base_url,
            api_key,
            match_model,
            pair_model,
        }
    }

    /// One chat-completion turn; returns the assistant message content.
    async fn complete(&self, model: &str, prompt: String) -> anyhow::Result<String> {
        let response = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": model,
                "temperature": 0,
                "messages": [{ "role": "user", "content": prompt }],
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<ChatResponse>()
            .await?;

        response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .ok_or_else(|| anyhow::anyhow!("model returned no choices"))
    }
}

#[async_trait]
impl ModelClient for GeminiClient {
    async fn match_score(&self, resume: &str, job: &JobText) -> anyhow::Result<MatchScore> {
        let prompt = format!(
            "Rate how well this candidate fits the role on a 0-100 scale.\n\
             Reply with ONLY JSON: {{\"score\": <0-100>, \"why\": \"<one line>\"}}.\n\n\
             RESUME:\n{resume}\n\nROLE:\n{} — {}",
            job.title, job.summary
        );
        let raw = self.complete(&self.match_model, prompt).await?;
        let parsed: MatchReply = serde_json::from_str(extract_json(&raw))?;
        Ok(MatchScore {
            score: parsed.score.clamp(0.0, 100.0),
            why: parsed.why,
        })
    }

    async fn compare(&self, resume: &str, a: &JobText, b: &JobText) -> anyhow::Result<Ordering> {
        let prompt = format!(
            "Which role fits the resume better?\n\
             Reply with ONLY JSON: {{\"winner\": \"A\" | \"B\" | \"tie\"}}.\n\n\
             RESUME:\n{resume}\n\nROLE A:\n{} — {}\n\nROLE B:\n{} — {}",
            a.title, a.summary, b.title, b.summary
        );
        let raw = self.complete(&self.pair_model, prompt).await?;
        let parsed: CompareReply = serde_json::from_str(extract_json(&raw))?;
        Ok(match parsed.winner.trim().to_ascii_uppercase().as_str() {
            "A" => Ordering::Greater,
            "B" => Ordering::Less,
            _ => Ordering::Equal,
        })
    }
}

/// Models sometimes wrap JSON in prose or fences; take the outermost `{...}`.
fn extract_json(raw: &str) -> &str {
    match (raw.find('{'), raw.rfind('}')) {
        (Some(start), Some(end)) if end > start => &raw[start..=end],
        _ => raw,
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

#[derive(Deserialize)]
struct MatchReply {
    score: f32,
    #[serde(default)]
    why: String,
}

#[derive(Deserialize)]
struct CompareReply {
    winner: String,
}

#[cfg(test)]
mod tests {
    use super::extract_json;

    #[test]
    fn extracts_json_from_fenced_or_noisy_output() {
        assert_eq!(
            extract_json("```json\n{\"score\":80}\n```"),
            "{\"score\":80}"
        );
        assert_eq!(
            extract_json("here: {\"winner\":\"A\"} ok"),
            "{\"winner\":\"A\"}"
        );
        assert_eq!(extract_json("no json"), "no json");
    }
}
