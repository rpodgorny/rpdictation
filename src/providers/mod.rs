use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn transcribe(&self, audio_data: &[u8], sample_rate: u32) -> Result<String>;
    fn cost_per_minute(&self) -> Option<f64>;
}

pub const API_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub mod google;
pub mod openai;
