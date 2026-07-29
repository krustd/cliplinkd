use anyhow::Context;
use arboard::ImageData;
use std::borrow::Cow;
use std::path::Path;

/// Write text to the system clipboard.
pub fn set_text(text: &str) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("Failed to open system clipboard")?;
    clipboard
        .set_text(text)
        .context("Failed to write to clipboard")?;
    Ok(())
}

/// Decode an image file and place its pixels onto the system clipboard.
pub fn set_image_file(path: &Path) -> anyhow::Result<()> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read image file: {}", path.display()))?;
    let image = image::load_from_memory(&bytes).context("Failed to decode image")?;
    let rgba = image.to_rgba8();
    let image_data = ImageData {
        width: rgba.width() as usize,
        height: rgba.height() as usize,
        bytes: Cow::Owned(rgba.into_raw()),
    };
    let mut clipboard = arboard::Clipboard::new().context("Failed to open system clipboard")?;
    clipboard
        .set_image(image_data)
        .context("Failed to write image to clipboard")?;
    Ok(())
}

/// Write local file paths to the system clipboard as file references.
pub fn set_files(paths: &[impl AsRef<Path>]) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("Failed to open system clipboard")?;
    clipboard
        .set()
        .file_list(paths)
        .context("Failed to write files to clipboard")?;
    Ok(())
}
