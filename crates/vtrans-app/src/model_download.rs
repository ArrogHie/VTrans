//! Translation model download orchestration.
//!
//! `download_translation_model` streams `translation.model.download_url`
//! into `{data}/models/translation/model.onnx.part` (resuming an existing
//! `.part` via the `Range` header), verifies the SHA-256 from the manifest,
//! and atomically renames the part into `model.onnx`. Progress is throttled
//! (at most every 500 ms or every 1 MiB) and forwarded through the
//! `model_download_progress` event; the promise only resolves when the
//! download finishes, fails, or is cancelled.
//!
//! The IO-heavy verify/rename step lives in [`finalize_download`] and runs
//! on the blocking pool so the async runtime never stalls on hashing a
//! 400 MB file.

use std::path::{Path, PathBuf};

use futures::StreamExt;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::error::AppError;
use crate::events::emit_model_download_progress;
use crate::state::AppState;

/// Minimum interval between two `model_download_progress` events.
const PROGRESS_THROTTLE_MS: u64 = 500;
/// Minimum transferred bytes between two `model_download_progress` events.
const PROGRESS_THROTTLE_BYTES: u64 = 1024 * 1024;
/// Sha-256 read buffer size.
const HASH_BUFFER_SIZE: usize = 8192;
/// How long `delete_translation_model` waits for a cancelled download task
/// to release the download slot before deleting files anyway.
pub(crate) const DOWNLOAD_CANCEL_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Returns the `.part` sidecar path used during a download.
///
/// # Example
///
/// ```
/// use std::path::Path;
/// use vtrans_app::model_download::part_path_for;
///
/// assert_eq!(
///     part_path_for(Path::new(r"D:\VTrans\data\models\translation\model.onnx")),
///     Path::new(r"D:\VTrans\data\models\translation\model.onnx.part")
/// );
/// ```
#[must_use]
pub fn part_path_for(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

/// Decides whether a progress event should be emitted for the current
/// download state.
///
/// The event fires when the download is complete, when at least
/// [`PROGRESS_THROTTLE_BYTES`] arrived since the last event, or when
/// [`PROGRESS_THROTTLE_MS`] elapsed with some progress made. Kept pure for
/// unit tests.
#[must_use]
pub(crate) fn should_emit_progress(
    elapsed_ms: u64,
    bytes_since_last: u64,
    bytes: u64,
    total: u64,
) -> bool {
    let completed = total > 0 && bytes >= total;
    completed
        || bytes_since_last >= PROGRESS_THROTTLE_BYTES
        || (elapsed_ms >= PROGRESS_THROTTLE_MS && bytes_since_last > 0)
}

/// Rejects a provider switch to `"local"` while a translation model
/// download is in progress.
///
/// The frontend disables the switch too; this backend check is the second
/// line of defense so a stale UI can never race the download.
pub(crate) fn reject_local_switch_during_download(
    download_active: bool,
    provider_id: &str,
) -> Result<(), AppError> {
    if download_active && provider_id == "local" {
        return Err(AppError::ModelDownload(
            "翻译模型正在下载，暂不能切换到本地翻译引擎".to_string(),
        ));
    }
    Ok(())
}

/// Verifies the downloaded `.part` file and atomically installs it.
///
/// A SHA-256 mismatch deletes the `.part` (a fresh download starts over
/// next time) and returns a clear error — the corrupt bytes are **never**
/// renamed over `model.onnx`. On success the part is atomically renamed
/// into the target (replacing a previous file on Windows). Runs on the
/// blocking pool because it hashes the full download.
pub(crate) fn finalize_download(
    part_path: &Path,
    target_path: &Path,
    expected_sha256: &str,
) -> Result<(), AppError> {
    let actual = match sha256_file(part_path) {
        Ok(hash) => hash,
        Err(error) => {
            let message = format!("无法读取下载内容进行校验: {error}");
            warn!(error = %message, "downloaded model file is unreadable");
            let _ = std::fs::remove_file(part_path);
            return Err(AppError::ModelDownload(message));
        }
    };
    if actual != expected_sha256 {
        warn!(
            expected = %expected_sha256,
            actual = %actual,
            "downloaded translation model sha256 mismatch"
        );
        if let Err(error) = std::fs::remove_file(part_path) {
            warn!(error = %error, path = %part_path.display(), "failed to remove the mismatched part file");
        }
        return Err(AppError::ModelDownload(
            "下载文件 sha256 校验失败，已丢弃，请重新下载".to_string(),
        ));
    }
    // On Windows `std::fs::rename` replaces an existing destination, so a
    // re-download atomically swaps the model file.
    std::fs::rename(part_path, target_path)
        .map_err(|error| AppError::ModelDownload(format!("下载完成但重命名失败: {error}")))?;
    info!(target = %target_path.display(), "translation model installed");
    Ok(())
}

/// Computes the lowercase hex SHA-256 of a file with a small fixed buffer.
fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; HASH_BUFFER_SIZE];
    loop {
        let read = std::io::Read::read(&mut reader, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Streams the download URL into `part_path`, resuming an existing part.
///
/// Returns `(downloaded_bytes, total_bytes)`. When the `.part` exists the
/// request carries a `Range` header and only a `206` response appends; a
/// `200` response restarts from zero. Any other status aborts with a clear
/// error. Cancellation stops the transfer and **keeps** the part so a later
/// download resumes.
async fn stream_to_part(
    client: &reqwest::Client,
    url: &str,
    part_path: &Path,
    total_hint: u64,
    cancel: &CancellationToken,
    app: &AppHandle,
) -> Result<(u64, u64), AppError> {
    let existing_len = tokio::fs::metadata(part_path)
        .await
        .map_or(0, |metadata| metadata.len());
    let resume = existing_len > 0;

    let mut request = client.get(url);
    if resume {
        request = request.header(reqwest::header::RANGE, format!("bytes={existing_len}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|error| AppError::ModelDownload(format!("下载请求失败: {error}")))?;
    let status = response.status().as_u16();
    let append = match status {
        206 => true,
        200 => false,
        other => {
            return Err(AppError::ModelDownload(format!("下载失败: HTTP {other}")));
        }
    };
    if resume && !append {
        tokio::fs::remove_file(part_path)
            .await
            .map_err(|error| AppError::ModelDownload(format!("清理旧下载文件失败: {error}")))?;
    }

    // Total size: Content-Range total (resume), Content-Length (fresh), or
    // the manifest's `download_size_bytes` as the last resort.
    let content_range_total = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range_total);
    let content_length = response.content_length();
    let total = content_range_total
        .or(content_length)
        .filter(|value| *value > 0)
        .unwrap_or(total_hint);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part_path)
        .await
        .map_err(|error| AppError::ModelDownload(format!("无法写入下载文件: {error}")))?;
    let mut downloaded = if append { existing_len } else { 0 };
    emit_model_download_progress(app, downloaded, total, fraction(downloaded, total));

    let mut stream = response.bytes_stream();
    let mut last_emit = tokio::time::Instant::now();
    let mut last_emit_bytes = downloaded;
    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                if let Err(error) = file.flush().await {
                    warn!(error = %error, "failed to flush the partial download before cancelling");
                }
                info!(bytes = downloaded, resume = true, "translation model download cancelled; part file kept for resume");
                return Err(AppError::ModelDownload("下载已取消".to_string()));
            }
            chunk = stream.next() => {
                let Some(chunk) = chunk else { break };
                let chunk = chunk.map_err(|error| {
                    AppError::ModelDownload(format!("下载数据流中断: {error}"))
                })?;
                file.write_all(&chunk)
                    .await
                    .map_err(|error| AppError::ModelDownload(format!("写入下载文件失败: {error}")))?;
                downloaded += chunk.len() as u64;
                let elapsed_ms =
                    u64::try_from(last_emit.elapsed().as_millis()).unwrap_or(u64::MAX);
                if should_emit_progress(
                    elapsed_ms,
                    downloaded - last_emit_bytes,
                    downloaded,
                    total,
                ) {
                    emit_model_download_progress(app, downloaded, total, fraction(downloaded, total));
                    last_emit = tokio::time::Instant::now();
                    last_emit_bytes = downloaded;
                }
            }
        }
    }
    file.flush()
        .await
        .map_err(|error| AppError::ModelDownload(format!("刷新下载文件失败: {error}")))?;
    drop(file);
    Ok((downloaded, total))
}

/// Parses the total out of a `Content-Range` header (`bytes 0-99/100`).
fn parse_content_range_total(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.trim().parse::<u64>().ok()
}

/// Clamps `bytes / total` into `[0.0, 1.0]`; `0.0` while the total is unknown.
///
/// The value is display-only, so the lossy narrowing through `f64` is
/// intentional and acceptable (a progress bar needs no 64-bit precision).
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn fraction(bytes: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        ((bytes as f64 / total as f64).clamp(0.0, 1.0)) as f32
    }
}

/// Runs the whole translation-model download flow for
/// [`crate::commands::download_translation_model`].
///
/// The caller has already registered `cancel` in the shared download slot.
/// The promise resolves on completion, failure, or cancellation; the slot
/// is released by the caller afterwards. On success the model status
/// snapshot is refreshed and a configured local provider is rebuilt.
#[tracing::instrument(skip(state, app, cancel))]
pub(crate) async fn run_translation_model_download(
    state: &AppState,
    app: &AppHandle,
    cancel: CancellationToken,
) -> Result<(), AppError> {
    let Some(manager) = state.model_manager() else {
        return Err(AppError::ModelNotReady(
            "模型清单不可用，无法下载翻译模型".to_string(),
        ));
    };
    let entry = manager
        .manifest()
        .translation
        .as_ref()
        .map(|group| group.model.clone())
        .ok_or_else(|| AppError::ModelNotReady("翻译模型条目未配置，无法下载".to_string()))?;
    let url = entry
        .download_url
        .clone()
        .ok_or_else(|| AppError::ModelDownload("模型清单未提供下载地址".to_string()))?;
    // Logging discipline: only the host is recorded, never a full (possibly
    // signed) URL.
    let host = reqwest::Url::parse(&url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .unwrap_or_else(|| "<unknown-host>".to_string());
    info!(host = %host, entry_id = %entry.id, "translation model download started");

    let data_models_dir = state.data_models_dir().to_path_buf();
    let target_path = data_models_dir.join(&entry.path);
    let part_path = part_path_for(&target_path);
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| AppError::ModelDownload(format!("无法创建模型目录: {error}")))?;
    }
    let total_hint = entry.download_size_bytes.unwrap_or(entry.size_bytes);
    let expected_sha256 = entry.sha256.clone();

    let client = reqwest::Client::builder()
        .build()
        .map_err(|error| AppError::ModelDownload(format!("无法创建下载客户端: {error}")))?;
    let (downloaded, total) =
        stream_to_part(&client, &url, &part_path, total_hint, &cancel, app).await?;
    info!(
        host = %host,
        bytes = downloaded,
        total,
        "translation model download finished; verifying sha256"
    );

    let verify_part = part_path.clone();
    let verify_target = target_path.clone();
    let verify_sha256 = expected_sha256;
    tokio::task::spawn_blocking(move || {
        finalize_download(&verify_part, &verify_target, &verify_sha256)
    })
    .await
    .map_err(|error| AppError::Tauri(format!("模型校验任务失败: {error}")))??;

    emit_model_download_progress(app, total, total, 1.0);
    if let Err(error) = state.refresh_model_status_async().await {
        warn!(error = %error, "model status refresh after download failed");
    }
    if let Err(error) = state
        .rebuild_translation_provider_after_model_change(app)
        .await
    {
        warn!(error = %error, "translation provider rebuild after download failed");
    }
    info!(entry_id = %entry.id, "translation model download completed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// Minimal std-only temporary-directory guard (parallel-test safe).
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "vtrans-app-download-{name}-{}-{seq}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir should be created");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn part_path_appends_the_part_suffix() {
        assert_eq!(
            part_path_for(Path::new("models/translation/model.onnx")),
            Path::new("models/translation/model.onnx.part")
        );
    }

    #[test]
    fn progress_throttle_fires_on_completion_bytes_and_time() {
        // Completion always emits.
        assert!(should_emit_progress(0, 0, 100, 100));
        // A full throttle quantum emits.
        assert!(should_emit_progress(
            10,
            PROGRESS_THROTTLE_BYTES,
            5_000_000,
            10_000_000
        ));
        // Time threshold with progress emits.
        assert!(should_emit_progress(
            PROGRESS_THROTTLE_MS,
            1,
            100,
            10_000_000
        ));
        // Below both thresholds: silent.
        assert!(!should_emit_progress(10, 1, 100, 10_000_000));
        // No progress: silent even after the time threshold.
        assert!(!should_emit_progress(
            PROGRESS_THROTTLE_MS,
            0,
            100,
            10_000_000
        ));
        // Unknown total never reports completion on its own.
        assert!(!should_emit_progress(0, 0, 0, 0));
    }

    #[test]
    fn local_switch_is_rejected_only_during_an_active_download() {
        assert!(
            reject_local_switch_during_download(true, "local").is_err(),
            "switching to local during a download must be rejected"
        );
        // Cloud switches stay allowed while a download runs.
        for provider in ["openai", "deepl", "google", "azure", "baidu"] {
            assert!(reject_local_switch_during_download(true, provider).is_ok());
        }
        // Nothing is rejected once the download is over.
        assert!(reject_local_switch_during_download(false, "local").is_ok());
    }

    #[test]
    fn finalize_installs_a_matching_part_atomically() {
        let dir = TestDir::new("finalize-ok");
        let part = dir.path().join("model.onnx.part");
        let target = dir.path().join("model.onnx");
        std::fs::write(&part, b"model-bytes").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"model-bytes"));

        finalize_download(&part, &target, &expected).unwrap();
        assert!(
            !part.exists(),
            "the part file must be consumed by the rename"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"model-bytes");
    }

    #[test]
    fn finalize_replaces_an_existing_target() {
        let dir = TestDir::new("finalize-replace");
        let part = dir.path().join("model.onnx.part");
        let target = dir.path().join("model.onnx");
        std::fs::write(&part, b"fresh-model").unwrap();
        std::fs::write(&target, b"stale-model").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"fresh-model"));

        finalize_download(&part, &target, &expected).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"fresh-model");
    }

    #[test]
    fn finalize_rolls_back_a_hash_mismatch_without_touching_the_target() {
        let dir = TestDir::new("finalize-mismatch");
        let part = dir.path().join("model.onnx.part");
        let target = dir.path().join("model.onnx");
        std::fs::write(&part, b"corrupted-download").unwrap();
        std::fs::write(&target, b"existing-good-model").unwrap();

        let error = finalize_download(
            &part,
            &target,
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap_err();
        assert!(matches!(error, AppError::ModelDownload(_)));
        assert!(error.to_string().contains("sha256"));
        // The mismatched part is cleaned up and the old model is untouched.
        assert!(!part.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"existing-good-model");
    }

    #[test]
    fn finalize_reports_unreadable_parts_and_cleans_up() {
        let dir = TestDir::new("finalize-unreadable");
        let part = dir.path().join("model.onnx.part");
        let target = dir.path().join("model.onnx");
        std::fs::write(&part, b"bytes").unwrap();
        // Make the part unreadable by replacing it with a directory.
        std::fs::remove_file(&part).unwrap();
        std::fs::create_dir(&part).unwrap();

        let error = finalize_download(
            &part,
            &target,
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap_err();
        assert!(matches!(error, AppError::ModelDownload(_)));
        assert!(!target.exists());
    }

    #[test]
    fn content_range_total_parses_standard_and_unknown_forms() {
        assert_eq!(parse_content_range_total("bytes 0-99/100"), Some(100));
        assert_eq!(
            parse_content_range_total("bytes */403368390"),
            Some(403_368_390)
        );
        assert_eq!(parse_content_range_total("garbage"), None);
    }

    #[test]
    fn fraction_is_clamped_and_zero_for_unknown_total() {
        assert!((fraction(0, 0) - 0.0).abs() < f32::EPSILON);
        assert!((fraction(5, 10) - 0.5).abs() < f32::EPSILON);
        assert!((fraction(10, 10) - 1.0).abs() < f32::EPSILON);
        // A short stream (e.g. server miscount) must not exceed 1.0.
        assert!((fraction(11, 10) - 1.0).abs() < f32::EPSILON);
    }
}
