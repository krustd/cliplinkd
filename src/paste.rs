use enigo::{Enigo, Key, KeyboardControllable};

/// Simulate a paste operation (Cmd+V on macOS, Ctrl+V elsewhere).
pub fn simulate_paste() -> anyhow::Result<()> {
    let mut enigo = Enigo::new();

    #[cfg(target_os = "macos")]
    {
        enigo.key_down(Key::Meta);
        enigo.key_click(Key::Layout('v'));
        enigo.key_up(Key::Meta);
    }

    #[cfg(not(target_os = "macos"))]
    {
        enigo.key_down(Key::Control);
        enigo.key_click(Key::Layout('v'));
        enigo.key_up(Key::Control);
    }

    Ok(())
}
