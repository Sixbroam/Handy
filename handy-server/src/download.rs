use anyhow::{anyhow, Result};
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

/// Download a single file from a URL with Range-based resume support.
pub async fn download_file(
    url: &str,
    dest: &Path,
    expected_size: Option<u64>,
) -> Result<()> {
    let partial_path = dest.with_extension("part");

    // Check if already fully downloaded
    if dest.exists() {
        let actual = dest.metadata()?.len();
        if let Some(exp) = expected_size {
            if actual == exp {
                tracing::info!("File already complete: {}", dest.display());
                return Ok(());
            }
        } else {
            tracing::info!("File already exists: {}", dest.display());
            return Ok(());
        }
    }

    // Determine resume point
    let mut resume_from: u64 = 0;
    if partial_path.exists() {
        resume_from = partial_path.metadata()?.len();
        tracing::info!("Resuming download from byte {}", resume_from);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(30))
        .build()?;

    // Start request with optional Range header
    let mut request = client.get(url);
    if resume_from > 0 {
        request = request.header("Range", format!("bytes={}-", resume_from));
    }

    let mut response = request.send().await?;

    // If server doesn't support ranges, restart from scratch
    if resume_from > 0 && response.status() == reqwest::StatusCode::OK {
        tracing::warn!("Server doesn't support Range, restarting download");
        let _ = fs::remove_file(&partial_path);
        resume_from = 0;

        response = client.get(url).send().await?;
    }

    if !response.status().is_success()
        && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
    {
        return Err(anyhow!(
            "Download failed: HTTP {}",
            response.status()
        ));
    }

    let total_size = if resume_from > 0 {
        resume_from + response.content_length().unwrap_or(0)
    } else {
        response.content_length().unwrap_or(0)
    };

    // Verify expected size
    if let Some(exp) = expected_size {
        if total_size > 0 && total_size != exp {
            // Content-Length may not be available, continue anyway
            tracing::warn!(
                "Size mismatch: expected {} but server reports {}",
                exp,
                total_size
            );
        }
    }

    let mut file = if resume_from > 0 {
        OpenOptions::new().create(true).append(true).open(&partial_path)?
    } else {
        File::create(&partial_path)?
    };

    let mut downloaded = resume_from as i64;
    let mut last_update = Instant::now();

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as i64;

        // Progress logging (throttled to ~1/sec)
        if last_update.elapsed() >= Duration::from_secs(1) {
            let elapsed = last_update.elapsed().as_secs_f64();
            let speed = chunk.len() as f64 / elapsed;
            let pct = if total_size > 0 {
                downloaded as f64 / total_size as f64 * 100.0
            } else {
                0.0
            };
            eprintln!(
                "\r  Download: {:.1}% ({}/{} MB, {:.1} MB/s)",
                pct,
                downloaded as f64 / 1024.0 / 1024.0,
                total_size as f64 / 1024.0 / 1024.0,
                speed / 1024.0 / 1024.0,
            );
            io::stdout().flush().ok();
            last_update = Instant::now();
        }
    }

    file.flush()?;
    drop(file);

    // Final progress
    eprintln!();

    // Verify final size
    if let Some(exp) = expected_size {
        let actual = partial_path.metadata()?.len();
        if actual != exp {
            let _ = fs::remove_file(&partial_path);
            return Err(anyhow!(
                "Download incomplete: expected {} bytes, got {}",
                exp,
                actual
            ));
        }
    }

    // Atomic rename
    fs::rename(&partial_path, dest)?;
    tracing::info!("Download complete: {}", dest.display());
    Ok(())
}

/// Download a tar.gz archive and extract it to the target directory.
#[allow(dead_code)]
pub async fn download_and_extract(
    url: &str,
    dest_dir: &Path,
) -> Result<()> {
    let temp_archive = dest_dir.with_extension("part");
    download_file(url, &temp_archive, None).await?;

    // Prepare extraction directory
    let temp_extract = dest_dir.with_extension("extracting");
    if temp_extract.exists() {
        fs::remove_dir_all(&temp_extract)?;
    }
    fs::create_dir_all(&temp_extract)?;

    // Extract
    tracing::info!("Extracting archive to {}", temp_extract.display());
    let tar_gz = File::open(&temp_archive)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    archive.unpack(&temp_extract)?;

    // Find extracted directory
    let extracted_dirs: Vec<_> = fs::read_dir(&temp_extract)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .collect();

    if extracted_dirs.len() == 1 {
        let source = &extracted_dirs[0].path();
        if dest_dir.exists() {
            fs::remove_dir_all(dest_dir)?;
        }
        fs::rename(source, dest_dir)?;
    } else {
        if dest_dir.exists() {
            fs::remove_dir_all(dest_dir)?;
        }
        fs::rename(&temp_extract, dest_dir)?;
    }

    // Cleanup
    let _ = fs::remove_file(&temp_archive);
    let _ = fs::remove_dir_all(&temp_extract);

    tracing::info!("Extraction complete: {}", dest_dir.display());
    Ok(())
}

/// Download a GGUF model from HuggingFace.
pub async fn download_gguf_from_hf(
    repo_id: &str,
    filename: &str,
    dest: &Path,
    expected_size: Option<u64>,
) -> Result<()> {
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        repo_id, filename
    );
    download_file(&url, dest, expected_size).await
}

/// Download an ONNX model archive from blob.handy.computer.
#[allow(dead_code)]
pub async fn download_onnx_archive(
    slug: &str,
    url: &str,
    dest_dir: &Path,
) -> Result<()> {
    tracing::info!("Downloading ONNX model {} from {}", slug, url);
    download_and_extract(url, dest_dir).await
}
