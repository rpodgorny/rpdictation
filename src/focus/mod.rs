use anyhow::Result;
use async_trait::async_trait;

/// Opaque window identifier (compositor-specific)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowId(pub String);

#[async_trait]
pub trait FocusProvider: Send + Sync {
    /// Get the currently focused window ID
    async fn get_focused_window(&self) -> Result<Option<WindowId>>;

    /// Set focus to a specific window
    async fn set_focused_window(&self, window_id: &WindowId) -> Result<bool>;

    /// Provider name for logging/debugging
    fn name(&self) -> &str;
}

#[cfg(unix)]
pub mod niri;

/// Detect and create the appropriate focus provider for the current compositor
pub async fn detect_focus_provider() -> Option<Box<dyn FocusProvider>> {
    // Try niri first (Unix only)
    #[cfg(unix)]
    {
        if let Some(provider) = niri::NiriFocusProvider::detect().await {
            return Some(Box::new(provider));
        }
    }

    // Future: add more compositors here (hyprland, sway, etc.)

    None
}
