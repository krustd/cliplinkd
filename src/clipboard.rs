use anyhow::Context;

/// Write text to the system clipboard.
pub fn set_text(text: &str) -> anyhow::Result<()> {
    let mut clipboard =
        arboard::Clipboard::new().context("Failed to open system clipboard")?;
    clipboard
        .set_text(text)
        .context("Failed to write to clipboard")?;
    Ok(())
}
