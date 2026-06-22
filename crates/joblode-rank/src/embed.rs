//! Query embedding for semantic search. Turns a free-text description into a
//! vector in the *same* space as the dataset's stored embeddings (OpenAI
//! `text-embedding-3-small`, 1536-d), so cosine similarity is meaningful.
//!
//! The trait is the seam we mock in tests; [`OpenAiEmbedder`] is the thin
//! production client (HTTP + JSON, like [`crate::GeminiClient`]).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

/// OpenAI's embeddings base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// The embedding model matching the dataset's vectors.
pub const DEFAULT_MODEL: &str = "text-embedding-3-small";

/// Embeds query text into a vector compatible with the corpus.
#[async_trait]
pub trait EmbedClient: Send + Sync {
    /// Returns the embedding of `text`.
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
}

/// Outbound request timeout, so an upstream stall can't hang a search.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Thin OpenAI-compatible embeddings client.
#[derive(Clone)]
pub struct OpenAiEmbedder {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl std::fmt::Debug for OpenAiEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the API key.
        f.debug_struct("OpenAiEmbedder")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl OpenAiEmbedder {
    /// Builds a client. `base_url`/`model` empty fall back to the defaults.
    #[must_use]
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        let base_url = if base_url.trim().is_empty() {
            DEFAULT_BASE_URL.to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        let model = if model.trim().is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            model
        };
        Self {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client builds with a timeout"),
            base_url,
            api_key,
            model,
        }
    }
}

#[async_trait]
impl EmbedClient for OpenAiEmbedder {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let response = self
            .http
            .post(format!("{}/embeddings", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&json!({ "model": self.model, "input": text }))
            .send()
            .await?
            .error_for_status()?
            .json::<EmbedResponse>()
            .await?;

        response
            .data
            .into_iter()
            .next()
            .map(|item| item.embedding)
            .ok_or_else(|| anyhow::anyhow!("embeddings response had no data"))
    }
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}
