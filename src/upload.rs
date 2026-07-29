use anyhow::{anyhow, bail, Context};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, BufWriter};

pub const MAX_CHUNK_SIZE: usize = 48 * 1024;
const CACHE_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadKind {
    Image,
    File,
}

impl UploadKind {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "image" => Ok(Self::Image),
            "file" => Ok(Self::File),
            _ => bail!("不支持的上传类型"),
        }
    }
}

pub struct Upload {
    id: String,
    kind: UploadKind,
    filename: String,
    expected_size: u64,
    received_size: u64,
    next_seq: u64,
    hasher: Sha256,
    temp_path: PathBuf,
    file: Option<BufWriter<File>>,
    cleanup_on_drop: bool,
}

pub struct CompletedUpload {
    pub kind: UploadKind,
    pub filename: String,
    pub path: PathBuf,
}

impl Upload {
    pub async fn start(
        id: String,
        kind: UploadKind,
        filename: String,
        expected_size: u64,
        max_receive_size: usize,
    ) -> anyhow::Result<Self> {
        if id.is_empty() || id.len() > 128 {
            bail!("无效的传输 ID");
        }
        if expected_size == 0 {
            bail!("不能发送空文件");
        }
        if expected_size > max_receive_size as u64 {
            bail!("文件超过接收上限 (最大 {} 字节)", max_receive_size);
        }

        let filename = sanitize_filename(&filename)?;
        let root = upload_root()?;
        tokio::fs::create_dir_all(&root)
            .await
            .context("无法创建文件缓存目录")?;
        cleanup_old_uploads(&root).await;
        let temp_path = root.join(format!(".{}.part", uuid::Uuid::new_v4()));
        let file = File::create(&temp_path)
            .await
            .with_context(|| format!("无法创建临时文件: {}", temp_path.display()))?;

        Ok(Self {
            id,
            kind,
            filename,
            expected_size,
            received_size: 0,
            next_seq: 0,
            hasher: Sha256::new(),
            temp_path,
            file: Some(BufWriter::new(file)),
            cleanup_on_drop: true,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn write_chunk(&mut self, seq: u64, payload_base64: &str) -> anyhow::Result<u64> {
        if seq != self.next_seq {
            bail!("数据块顺序错误");
        }
        let payload = base64::engine::general_purpose::STANDARD
            .decode(payload_base64)
            .map_err(|_| anyhow!("数据块不是有效的 Base64"))?;
        if payload.is_empty() || payload.len() > MAX_CHUNK_SIZE {
            bail!("数据块大小无效");
        }
        let next_size = self
            .received_size
            .checked_add(payload.len() as u64)
            .ok_or_else(|| anyhow!("文件大小溢出"))?;
        if next_size > self.expected_size {
            bail!("接收数据超过声明大小");
        }

        self.file
            .as_mut()
            .expect("upload file exists before completion")
            .write_all(&payload)
            .await
            .context("写入临时文件失败")?;
        self.hasher.update(&payload);
        self.received_size = next_size;
        self.next_seq += 1;
        Ok(self.received_size)
    }

    pub async fn finish(
        mut self,
        size_bytes: u64,
        sha256: &str,
    ) -> anyhow::Result<CompletedUpload> {
        if size_bytes != self.expected_size || self.received_size != self.expected_size {
            bail!("文件大小校验失败");
        }
        let actual_hash = format!("{:x}", self.hasher.clone().finalize());
        if !sha256.eq_ignore_ascii_case(&actual_hash) {
            bail!("文件完整性校验失败");
        }

        let mut file = self
            .file
            .take()
            .expect("upload file exists before completion");
        file.flush().await.context("刷新临时文件失败")?;
        file.get_ref()
            .sync_all()
            .await
            .context("同步临时文件失败")?;
        drop(file);

        let path = match self.kind {
            UploadKind::Image => self.temp_path.clone(),
            UploadKind::File => {
                let destination = unique_destination(&upload_root()?, &self.filename);
                tokio::fs::rename(&self.temp_path, &destination)
                    .await
                    .with_context(|| format!("无法完成文件保存: {}", destination.display()))?;
                destination
            }
        };
        self.cleanup_on_drop = false;

        Ok(CompletedUpload {
            kind: self.kind,
            filename: self.filename.clone(),
            path,
        })
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

pub fn remove_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("Failed to remove upload file {}: {}", path.display(), error);
        }
    }
}

fn upload_root() -> anyhow::Result<PathBuf> {
    dirs::cache_dir()
        .map(|dir| dir.join("cliplinkd").join("uploads"))
        .ok_or_else(|| anyhow!("无法定位系统缓存目录"))
}

fn sanitize_filename(filename: &str) -> anyhow::Result<String> {
    if filename.is_empty()
        || filename.len() > 255
        || filename == "."
        || filename == ".."
        || filename.contains(['/', '\\'])
        || filename.chars().any(char::is_control)
    {
        bail!("文件名无效");
    }
    Ok(filename.to_owned())
}

fn unique_destination(root: &Path, filename: &str) -> PathBuf {
    let candidate = root.join(filename);
    if !candidate.exists() {
        return candidate;
    }

    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let extension = path.extension().and_then(|value| value.to_str());
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let name = match extension {
        Some(extension) => format!("{}-{}.{}", stem, &suffix[..8], extension),
        None => format!("{}-{}", stem, &suffix[..8]),
    };
    root.join(name)
}

async fn cleanup_old_uploads(root: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let Ok(metadata) = entry.metadata().await else {
            continue;
        };
        let is_expired = metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > CACHE_RETENTION);
        if metadata.is_file() && is_expired {
            remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{remove_file, sanitize_filename, Upload, UploadKind};

    #[test]
    fn rejects_unsafe_filenames() {
        for filename in ["", ".", "..", "../../secret", "a/b", "a\\b", "bad\nname"] {
            assert!(sanitize_filename(filename).is_err(), "{filename}");
        }
    }

    #[tokio::test]
    async fn rejects_invalid_chunk_order_and_cleans_failed_upload() {
        let mut upload = Upload::start(
            uuid::Uuid::new_v4().to_string(),
            UploadKind::File,
            "cliplinkd-upload-test.txt".to_owned(),
            3,
            1024,
        )
        .await
        .unwrap();
        let temp_path = upload.temp_path.clone();
        assert!(upload.write_chunk(1, "YWJj").await.is_err());
        drop(upload);
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn removes_partial_file_when_digest_verification_fails() {
        let mut upload = Upload::start(
            uuid::Uuid::new_v4().to_string(),
            UploadKind::File,
            "cliplinkd-upload-test.txt".to_owned(),
            3,
            1024,
        )
        .await
        .unwrap();
        let temp_path = upload.temp_path.clone();
        upload.write_chunk(0, "YWJj").await.unwrap();
        assert!(upload.finish(3, "not-a-digest").await.is_err());
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn keeps_completed_image_until_clipboard_write() {
        let mut upload = Upload::start(
            uuid::Uuid::new_v4().to_string(),
            UploadKind::Image,
            "cliplinkd-upload-test.png".to_owned(),
            3,
            1024,
        )
        .await
        .unwrap();
        upload.write_chunk(0, "YWJj").await.unwrap();
        let completed = upload
            .finish(
                3,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            )
            .await
            .unwrap();
        assert!(completed.path.exists());
        remove_file(&completed.path);
    }
    #[test]
    fn keeps_valid_unicode_filename() {
        assert_eq!(sanitize_filename("照片 01.png").unwrap(), "照片 01.png");
    }
}
