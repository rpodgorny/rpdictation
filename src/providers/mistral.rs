use anyhow::{Context, Result};
use async_trait::async_trait;

use super::TranscriptionProvider;

pub struct MistralProvider {
    api_key: String,
}

impl MistralProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl TranscriptionProvider for MistralProvider {
    fn name(&self) -> &str {
        "Mistral"
    }

    async fn transcribe(&self, audio_data: &[u8], _sample_rate: u32) -> Result<String> {
        let client = reqwest::Client::new();
        let file_part = reqwest::multipart::Part::bytes(audio_data.to_vec())
            .file_name("recording.wav")
            .mime_str("audio/wav")?;
        let form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("model", "voxtral-mini-latest");

        println!("Sending request to Mistral API...");
        let response = client
            .post("https://api.mistral.ai/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .context("Failed to send request to Mistral API")?;

        println!("Got response with status: {}", response.status());
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("API error: {}", error_text));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse API response as JSON")?;

        let Some(text) = result["text"].as_str() else {
            anyhow::bail!("Failed to get transcription from response");
        };

        Ok(text.to_string())
    }

    fn cost_per_minute(&self) -> Option<f64> {
        Some(0.003)
    }
}
