use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    async fn transcribe(&self, audio_data: &[u8], sample_rate: u32) -> Result<String>;
    fn name(&self) -> &str;
    fn cost_per_minute(&self) -> Option<f64>;
}

pub mod google;
pub mod openai;
