use anyhow::{Context, Result};
use async_trait::async_trait;

use super::{FocusProvider, WindowId};

pub struct NiriFocusProvider;

impl NiriFocusProvider {
    /// Detect if niri compositor is available
    pub async fn detect() -> Option<Self> {
        // Check if niri msg command works
        let output = tokio::process::Command::new("niri")
            .args(["msg", "version"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .ok()?;

        if output.success() {
            Some(Self)
        } else {
            None
        }
    }
}

#[async_trait]
impl FocusProvider for NiriFocusProvider {
    async fn get_focused_window(&self) -> Result<Option<WindowId>> {
        let output = tokio::process::Command::new("niri")
            .args(["msg", "-j", "focused-window"])
            .output()
            .await
            .context("Failed to run niri msg focused-window")?;

        if !output.status.success() {
            // No focused window or command failed
            return Ok(None);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value =
            serde_json::from_str(&stdout).context("Failed to parse niri msg output")?;

        // Extract window ID from the JSON response
        if let Some(id) = json.get("id").and_then(|v| v.as_u64()) {
            Ok(Some(WindowId(id.to_string())))
        } else {
            Ok(None)
        }
    }

    async fn set_focused_window(&self, window_id: &WindowId) -> Result<bool> {
        let output = tokio::process::Command::new("niri")
            .args(["msg", "action", "focus-window", "--id", &window_id.0])
            .output()
            .await
            .context("Failed to run niri msg action focus-window")?;

        Ok(output.status.success())
    }

    fn name(&self) -> &str {
        "niri"
    }
}
