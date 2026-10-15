//! Bounded thumbnail-only work. URLs/tool output never enter public failures.
use super::*;
use image::{ImageFormat, ImageReader};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_BYTES: u64 = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThumbnailSaved {
    pub asset_id: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThumbnailFailure {
    Cancelled,
    Transport,
    Http(u16),
    TooLarge,
    Format,
    Storage,
    InvalidTarget,
}
impl ThumbnailFailure {
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport | Self::Http(408 | 425 | 429 | 500..=599)
        )
    }
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Transport => "transport",
            Self::Http(_) => "http",
            Self::TooLarge => "too_large",
            Self::Format => "invalid_image",
            Self::Storage => "storage",
            Self::InvalidTarget => "invalid_target",
        }
    }
    pub fn message(&self) -> String {
        match self {
            Self::Cancelled => "サムネ取得を取り消しました".into(),
            Self::Transport => "サムネの通信に失敗しました。再取得できます".into(),
            Self::Http(status) => format!("サムネを取得できませんでした（HTTP {status}）"),
            Self::TooLarge => "サムネ画像が容量上限を超えています".into(),
            Self::Format => "サムネを対応画像として読み込めませんでした".into(),
            Self::Storage => "サムネ画像を保存できませんでした".into(),
            Self::InvalidTarget => "サムネの取得先が対応していません".into(),
        }
    }
}

fn check(cancel: &Cancellation) -> Result<(), ThumbnailFailure> {
    cancel.check().map_err(|_| ThumbnailFailure::Cancelled)
}
fn retry<T>(
    cancel: &Cancellation,
    delays: &[Duration],
    mut operation: impl FnMut() -> Result<T, ThumbnailFailure>,
) -> Result<T, ThumbnailFailure> {
    for attempt in 0..=delays.len() {
        check(cancel)?;
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                let status = match error {
                    ThumbnailFailure::Http(n) => n,
                    _ => 0,
                };
                eprintln!(
                    "youtube_thumbnail reason={} http_status={} attempt={} retryable={}",
                    error.code(),
                    status,
                    attempt + 1,
                    error.retryable()
                );
                if !error.retryable() || attempt == delays.len() {
                    return Err(error);
                }
                let end = std::time::Instant::now() + delays[attempt];
                while std::time::Instant::now() < end {
                    check(cancel)?;
                    std::thread::sleep(
                        Duration::from_millis(50)
                            .min(end.saturating_duration_since(std::time::Instant::now())),
                    );
                }
            }
        }
    }
    unreachable!()
}

fn receive(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, ThumbnailFailure> {
    let response = client
        .get(url)
        .send()
        .map_err(|_| ThumbnailFailure::Transport)?;
    if !response.status().is_success() {
        return Err(ThumbnailFailure::Http(response.status().as_u16()));
    }
    if response.content_length().is_some_and(|n| n > MAX_BYTES) {
        return Err(ThumbnailFailure::TooLarge);
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ThumbnailFailure::Transport)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ThumbnailFailure::TooLarge);
    }
    Ok(bytes)
}

fn publish(
    root: &Path,
    id: &str,
    bytes: &[u8],
    cancel: &Cancellation,
) -> Result<ThumbnailSaved, ThumbnailFailure> {
    check(cancel)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ThumbnailFailure::TooLarge);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ThumbnailFailure::Format)?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP)
    ) {
        return Err(ThumbnailFailure::Format);
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| ThumbnailFailure::Format)?
        .thumbnail(480, 270);
    let folder = super::media::video_dir(root, id).map_err(|_| ThumbnailFailure::InvalidTarget)?;
    std::fs::create_dir_all(&folder).map_err(|_| ThumbnailFailure::Storage)?;
    let staged = folder.join("thumbnail.tmp");
    let result = (|| {
        image
            .save_with_format(&staged, ImageFormat::Jpeg)
            .map_err(|_| ThumbnailFailure::Storage)?;
        let encoded = std::fs::read(&staged).map_err(|_| ThumbnailFailure::Storage)?;
        let saved = ThumbnailSaved {
            asset_id: format!("thumbnail-{id}"),
            bytes: encoded.len() as u64,
        };
        check(cancel)?;
        std::fs::rename(&staged, folder.join("thumbnail.jpg"))
            .map_err(|_| ThumbnailFailure::Storage)?;
        Ok(saved)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(staged);
    }
    result
}

pub(crate) fn fetch_from(
    root: &Path,
    id: &str,
    url: &str,
    cancel: &Cancellation,
) -> Result<ThumbnailSaved, ThumbnailFailure> {
    if !super::valid_id(id) {
        return Err(ThumbnailFailure::InvalidTarget);
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| ThumbnailFailure::InvalidTarget)?;
    if parsed.scheme() != "https"
        || !matches!(parsed.host_str(), Some("i.ytimg.com" | "img.youtube.com"))
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
    {
        return Err(ThumbnailFailure::InvalidTarget);
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ThumbnailFailure::Transport)?;
    let bytes = retry(
        cancel,
        &[Duration::from_millis(500), Duration::from_millis(1500)],
        || receive(&client, url),
    )?;
    let result = publish(root, id, &bytes, cancel);
    if let Err(error) = &result {
        eprintln!(
            "youtube_thumbnail reason={} http_status=0 attempt=1 retryable=false",
            error.code()
        );
    }
    result
}

pub async fn run_thumbnail(
    root: PathBuf,
    id: String,
    cancel: Cancellation,
) -> Result<ThumbnailSaved, ThumbnailFailure> {
    tokio::task::spawn_blocking(move || {
        // Fixed public YouTube image endpoint; no metadata/video download.
        let url = format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg");
        fetch_from(&root, &id, &url, &cancel)
    })
    .await
    .map_err(|_| ThumbnailFailure::Storage)?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_retries_are_bounded_and_permanent_failures_do_not_repeat() {
        let cancel = Cancellation::default();
        for error in [
            ThumbnailFailure::Transport,
            ThumbnailFailure::Http(429),
            ThumbnailFailure::Http(503),
        ] {
            let mut count = 0;
            let result: Result<(), _> = retry(&cancel, &[Duration::ZERO, Duration::ZERO], || {
                count += 1;
                Err(error.clone())
            });
            assert_eq!(result, Err(error));
            assert_eq!(count, 3);
        }
        for error in [
            ThumbnailFailure::Http(403),
            ThumbnailFailure::Http(404),
            ThumbnailFailure::Format,
        ] {
            let mut count = 0;
            let _: Result<(), _> = retry(&cancel, &[Duration::ZERO, Duration::ZERO], || {
                count += 1;
                Err(error.clone())
            });
            assert_eq!(count, 1);
        }
        let mut count = 0;
        let result = retry(&cancel, &[Duration::ZERO], || {
            count += 1;
            if count == 1 {
                Err(ThumbnailFailure::Http(503))
            } else {
                Ok(7)
            }
        });
        assert_eq!(result, Ok(7));
        assert_eq!(count, 2);
    }
    #[test]
    fn validated_publish_accepts_webp_png_jpeg_and_preserves_other_media() {
        let temp = tempfile::tempdir().unwrap();
        let id = "abcdefghijk";
        let folder = temp.path().join("videos").join(id);
        std::fs::create_dir_all(folder.join("converted/keep")).unwrap();
        std::fs::write(folder.join("converted/keep/frame.jpg"), b"keep").unwrap();
        for format in [ImageFormat::WebP, ImageFormat::Png, ImageFormat::Jpeg] {
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(32, 24)
                .write_to(&mut bytes, format)
                .unwrap();
            let saved =
                publish(temp.path(), id, bytes.get_ref(), &Cancellation::default()).unwrap();
            assert_eq!(saved.asset_id, "thumbnail-abcdefghijk");
            assert_eq!(
                saved.bytes,
                std::fs::metadata(folder.join("thumbnail.jpg"))
                    .unwrap()
                    .len()
            );
        }
        let good = std::fs::read(folder.join("thumbnail.jpg")).unwrap();
        assert_eq!(
            publish(temp.path(), id, b"invalid", &Cancellation::default()).unwrap_err(),
            ThumbnailFailure::Format
        );
        let cancel = Cancellation::default();
        cancel.cancel();
        assert_eq!(
            publish(temp.path(), id, &good, &cancel).unwrap_err(),
            ThumbnailFailure::Cancelled
        );
        assert_eq!(std::fs::read(folder.join("thumbnail.jpg")).unwrap(), good);
        assert_eq!(
            std::fs::read(folder.join("converted/keep/frame.jpg")).unwrap(),
            b"keep"
        );
        assert!(!folder.join("thumbnail.tmp").exists());
    }
    #[test]
    fn bad_targets_and_error_messages_do_not_expose_urls() {
        assert_eq!(
            fetch_from(
                Path::new("/unused"),
                "abcdefghijk",
                "http://127.0.0.1/private?secret=x",
                &Cancellation::default()
            )
            .unwrap_err(),
            ThumbnailFailure::InvalidTarget
        );
        for error in [
            ThumbnailFailure::Transport,
            ThumbnailFailure::Http(404),
            ThumbnailFailure::Format,
        ] {
            assert!(!error.message().contains("http://"));
        }
    }
}
