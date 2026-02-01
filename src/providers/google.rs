use anyhow::{Context, Result};
use async_trait::async_trait;

use super::TranscriptionProvider;

pub struct GoogleProvider {
    api_key: String,
    language: String,
}

impl GoogleProvider {
    const DEFAULT_KEY: &str = "AIzaSyBOti4mM-6x9WDnZIjIeyEU21OpBXqWBgw";
    const ENDPOINT: &str = "http://www.google.com/speech-api/v2/recognize";

    pub fn new(api_key: Option<String>, language: String) -> Self {
        Self {
            api_key: api_key.unwrap_or(Self::DEFAULT_KEY.to_string()),
            language,
        }
    }
}

#[async_trait]
impl TranscriptionProvider for GoogleProvider {
    fn name(&self) -> &str {
        "Google"
    }

    async fn transcribe(&self, audio_data: &[u8], sample_rate: u32) -> Result<String> {
        // Convert WAV to FLAC (CPU-intensive, run in blocking thread)
        println!("Converting WAV to FLAC...");
        let audio_data_owned = audio_data.to_vec();
        let flac_data = tokio::task::spawn_blocking(move || {
            crate::audio::wav_to_flac(&audio_data_owned, sample_rate)
        })
        .await
        .context("FLAC encoding task panicked")??;

        // Send to Google API
        let client = reqwest::Client::new();
        let url = format!(
            "{}?key={}&lang={}&output=json",
            Self::ENDPOINT,
            self.api_key,
            self.language
        );

        println!("Sending request to Google Chromium Speech API...");
        let response = client
            .post(&url)
            .header(
                "Content-Type",
                format!("audio/x-flac; rate={}", sample_rate),
            )
            .body(flac_data)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .context("Failed to send request to Google API")?;

        println!("Got response with status: {}", response.status());
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("API error: {}", error_text));
        }

        // Parse newline-delimited JSON response
        let response_text = response.text().await?;

        // The response contains multiple JSON objects separated by newlines
        // We want the one with actual results (not just {"result":[]} empty)
        for line in response_text.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let json: serde_json::Value =
                serde_json::from_str(line).context("Failed to parse Google API response")?;

            if let Some(result_array) = json["result"].as_array() {
                if let Some(first_result) = result_array.first() {
                    if let Some(alternatives) = first_result["alternative"].as_array() {
                        if let Some(first_alt) = alternatives.first() {
                            if let Some(transcript) = first_alt["transcript"].as_str() {
                                return Ok(transcript.to_string());
                            }
                        }
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "No transcription found in Google API response"
        ))
    }

    fn cost_per_minute(&self) -> Option<f64> {
        None
    }
}
