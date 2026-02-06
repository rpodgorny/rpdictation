// Windows text typing via SendInput with KEYEVENTF_UNICODE.
// This file is only compiled on Windows (gated by #[cfg(windows)] mod declaration in main.rs).

use anyhow::Result;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_RETURN,
};

/// Send a pair of INPUT events (key down + key up) for a single Unicode code unit.
fn make_unicode_pair(code_unit: u16) -> [INPUT; 2] {
    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: code_unit,
                dwFlags: KEYEVENTF_UNICODE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: code_unit,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    [down, up]
}

/// Type a string into the currently focused window using SendInput with KEYEVENTF_UNICODE.
/// Handles all Unicode characters including those outside the BMP (via surrogate pairs).
/// Returns Ok(()) on success, or an error if SendInput fails.
pub fn type_text(text: &str) -> Result<()> {
    // Build all INPUT events at once for the entire string.
    // Each UTF-16 code unit needs a key-down + key-up pair.
    let mut inputs: Vec<INPUT> = Vec::new();

    for ch in text.chars() {
        let mut buf = [0u16; 2];
        let encoded = ch.encode_utf16(&mut buf);
        for &code_unit in encoded.iter() {
            let pair = make_unicode_pair(code_unit);
            inputs.push(pair[0]);
            inputs.push(pair[1]);
        }
    }

    if inputs.is_empty() {
        return Ok(());
    }

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        anyhow::bail!(
            "SendInput failed (returned 0). Text may not have been typed. \
            This can happen when typing into an elevated (UAC) window."
        );
    }
    if (sent as usize) < inputs.len() {
        eprintln!(
            "Warning: SendInput sent {}/{} events, some keystrokes may be missing",
            sent,
            inputs.len()
        );
    }

    Ok(())
}

/// Press the Enter key using SendInput with VK_RETURN.
pub fn press_enter() -> Result<()> {
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_RETURN,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_RETURN,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        anyhow::bail!("SendInput failed for Enter key");
    }

    Ok(())
}
