use anyhow::Context;
use base64::Engine;
use image::ImageEncoder;
use std::fs;
use std::path::PathBuf;

/// The kind of content currently on the system clipboard.
#[derive(Debug, Clone)]
pub enum ClipboardContent {
    Text(String),
    Image {
        width: usize,
        height: usize,
        /// RGBA pixel data, 4 bytes per pixel.
        rgba: Vec<u8>,
    },
    Files(Vec<FileEntry>),
    None,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Display name (filename only, e.g. "photo.png").
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// Absolute path to the file on disk.
    pub path: String,
}

// ── Public API ──────────────────────────────────────────────────────────

/// Read the current system clipboard content.
/// Priority: text → image → files → none.
pub fn read() -> anyhow::Result<ClipboardContent> {
    let mut clipboard = arboard::Clipboard::new().context("Failed to open system clipboard")?;

    // 1. Try files first — on macOS, file clipboard also exposes a text
    //    representation (the filename), so we must check files before text.
    if let Ok(files) = read_files() {
        if !files.is_empty() {
            return Ok(ClipboardContent::Files(files));
        }
    }

    // 2. Try image
    if let Ok(image_data) = clipboard.get_image() {
        let rgba = image_data.bytes.into_owned();
        return Ok(ClipboardContent::Image {
            width: image_data.width,
            height: image_data.height,
            rgba,
        });
    }

    // 3. Try text last
    if let Ok(text) = clipboard.get_text() {
        if !text.is_empty() {
            return Ok(ClipboardContent::Text(text));
        }
    }

    Ok(ClipboardContent::None)
}

/// Encode RGBA pixel data to PNG bytes.
pub fn encode_rgba_to_png(width: usize, height: usize, rgba: &[u8]) -> anyhow::Result<Vec<u8>> {
    let w = u32::try_from(width).context("image width overflow")?;
    let h = u32::try_from(height).context("image height overflow")?;

    let img =
        image::RgbaImage::from_raw(w, h, rgba.to_vec()).context("Invalid RGBA image dimensions")?;

    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(&img, w, h, image::ExtendedColorType::Rgba8)
        .context("Failed to encode PNG")?;
    Ok(png_bytes)
}

/// Encode arbitrary bytes to a base64 string.
pub fn encode_base64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Read a file from disk and return its raw bytes.
pub fn read_file_bytes(path: &str) -> anyhow::Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("Failed to read file: {}", path))
}

// ── File clipboard reading (platform-specific) ──────────────────────────

fn read_files() -> anyhow::Result<Vec<FileEntry>> {
    #[cfg(target_os = "macos")]
    {
        read_files_macos()
    }
    #[cfg(target_os = "linux")]
    {
        read_files_linux()
    }
    #[cfg(target_os = "windows")]
    {
        read_files_windows()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Ok(Vec::new())
    }
}

// ── macOS ───────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn read_files_macos() -> anyhow::Result<Vec<FileEntry>> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;

    unsafe {
        // NSPasteboard *pb = [NSPasteboard generalPasteboard];
        let pb: Retained<AnyObject> = msg_send![objc2::class!(NSPasteboard), generalPasteboard];

        // NSMutableArray *classes = [NSMutableArray arrayWithObject:[NSURL class]];
        let nsurl_class: &objc2::runtime::AnyClass = objc2::class!(NSURL);
        let classes: Retained<AnyObject> =
            msg_send![objc2::class!(NSMutableArray), arrayWithObject: nsurl_class];

        // NSArray<NSURL *> *urls = [pb readObjectsForClasses:classes options:nil];
        // Returns nil when no matching objects found.
        let urls: Option<Retained<AnyObject>> = msg_send![
            &*pb,
            readObjectsForClasses: &*classes,
            options: std::ptr::null::<AnyObject>()
        ];
        let urls = match urls {
            Some(u) => u,
            None => return Ok(Vec::new()),
        };

        let count: usize = msg_send![&*urls, count];
        let mut entries = Vec::with_capacity(count);

        for i in 0..count {
            let url: Retained<AnyObject> = msg_send![&*urls, objectAtIndex: i];
            // NSString *path = [url path];
            let path_obj: Option<Retained<AnyObject>> = msg_send![&*url, path];
            let path_obj = match path_obj {
                Some(p) => p,
                None => continue,
            };
            // const char *utf8 = [path UTF8String];
            let utf8: *const std::ffi::c_char = msg_send![&*path_obj, UTF8String];
            if !utf8.is_null() {
                let path_str = std::ffi::CStr::from_ptr(utf8).to_string_lossy().to_string();

                let path = PathBuf::from(&path_str);
                if let Ok(meta) = fs::metadata(&path) {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path_str.clone());
                    entries.push(FileEntry {
                        name,
                        size: meta.len(),
                        path: path_str,
                    });
                }
            }
        }
        Ok(entries)
    }
}

// ── Linux ───────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn read_files_linux() -> anyhow::Result<Vec<FileEntry>> {
    // On Linux, file managers typically set text/uri-list as a clipboard target.
    // arboard's get_text() may return the URI list for some backends.
    let text = match arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok())
    {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };

    parse_uri_list(&text)
}

/// Parse a text/uri-list string into FileEntry values.
fn parse_uri_list(text: &str) -> anyhow::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Handle file:// URI
        let path_str = if let Some(stripped) = line.strip_prefix("file://") {
            url_decode(stripped)
        } else if line.starts_with('/') {
            line.to_string()
        } else {
            continue;
        };

        let path = PathBuf::from(&path_str);
        if !path.exists() {
            continue;
        }

        if let Ok(meta) = fs::metadata(&path) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path_str.clone());
            entries.push(FileEntry {
                name,
                size: meta.len(),
                path: path_str,
            });
        }
    }

    Ok(entries)
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else {
            result.push(c);
        }
    }
    result
}

// ── Windows ─────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn read_files_windows() -> anyhow::Result<Vec<FileEntry>> {
    use windows::core::PWSTR;
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::UI::Shell::{DragQueryFileW, CF_HDROP, HDROP};

    unsafe {
        // OpenClipboard requires an owner window; None = current task
        if OpenClipboard(None).is_err() {
            return Ok(Vec::new());
        }

        let result = (|| -> anyhow::Result<Vec<FileEntry>> {
            let handle = GetClipboardData(CF_HDROP.0 as u32).context("No CF_HDROP on clipboard")?;

            let hdrop = HDROP(handle.0);
            // Pass 0xFFFFFFFF as file index to get the count
            let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);

            let mut entries = Vec::new();
            for i in 0..count {
                // First call to get required buffer size
                let len = DragQueryFileW(hdrop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                // Second call to get the path
                DragQueryFileW(hdrop, i, Some(PWSTR::from_raw(buf.as_mut_ptr())));
                let path_str = String::from_utf16_lossy(&buf[..len])
                    .trim_end_matches('\0')
                    .to_string();

                let path = PathBuf::from(&path_str);
                if let Ok(meta) = fs::metadata(&path) {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path_str.clone());
                    entries.push(FileEntry {
                        name,
                        size: meta.len(),
                        path: path_str,
                    });
                }
            }
            Ok(entries)
        })();

        let _ = CloseClipboard();
        result
    }
}
