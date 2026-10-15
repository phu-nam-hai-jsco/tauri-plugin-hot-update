//! Các hàm hỗ trợ nạp HTTP: nạp nội dung nhỏ có giới hạn (manifest, chữ ký) và
//! tải xuống tệp nén theo luồng stream có xác thực mã băm sha256 ngay trong quá trình tải.
//!
//! Tính toàn vẹn đến từ chuỗi chữ ký số chứ không phụ thuộc vào tầng truyền tải: tệp nén
//! phải khớp với mã băm sha256 được ghim chặt bên trong manifest *đã ký*, và số lượng byte
//! của nó phải khớp chính xác — luồng tải dài hơn dung lượng sẽ bị ngắt giữa chừng,
//! luồng tải ngắn hơn sẽ bị từ chối ở bước cuối. Không có cơ chế tiếp tục tải lại (resume):
//! bản tải dở dang hoặc không khớp sẽ bị hủy bỏ và tải lại từ đầu (đơn giản hóa thiết kế cho v1; tệp nén chỉ vài MB).
//!
//! ---
//!
//! HTTP fetch helpers: capped small-body fetches (manifest, signature) and
//! the streaming archive download with sha256-during-download verification.
//!
//! Integrity comes from the signature chain, not the transport: the archive
//! must hash to the sha256 pinned inside the *signed* manifest, and its byte
//! count must match exactly — a stream that runs long is aborted mid-flight,
//! one that runs short is refused at the end. There is no resume: a partial
//! or mismatched download is discarded and restarted from scratch (v1
//! simplicity; archives are a few MB).

use std::fmt::Write as _;
use std::fs::File;
use std::io::Write as _;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::manifest::ArchiveInfo;
use crate::{Error, Result};

/// Giới hạn an toàn cho nội dung manifest. Manifest thực tế chỉ khoảng vài trăm byte.
/// / Sanity cap for the manifest body. Real manifests are a few hundred bytes.
pub(crate) const MANIFEST_MAX_BYTES: u64 = 64 * 1024;

/// Giới hạn an toàn cho nội dung tệp `.minisig`. Chữ ký thực tế chỉ khoảng ~300 byte.
/// / Sanity cap for the `.minisig` body. Real signatures are ~300 bytes.
pub(crate) const SIGNATURE_MAX_BYTES: u64 = 8 * 1024;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Callback tiến độ: `(downloaded_bytes, total_bytes)`. Được gọi một lần cho mỗi
/// chunk nhận được; chuyển tiếp dưới dạng sự kiện tiến độ `download`.
///
/// ---
///
/// Progress callback: `(downloaded_bytes, total_bytes)`. Invoked once per
/// received chunk; WP4 forwards it as `download` progress events.
pub type OnProgress<'a> = &'a mut (dyn FnMut(u64, u64) + Send);

pub(crate) fn http_client() -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT);

    // reqwest's rustls features hard-depend on `rustls-platform-verifier`, and
    // with no explicit root store a client uses it. Its Android backend needs a
    // JNI context this plugin never initializes, which hangs the TLS handshake
    // there (no error, no response) — iOS/desktop platform verifiers are fine.
    // Hand reqwest a rustls config backed by the compiled-in Mozilla CA roots
    // so certificate verification is pure Rust and identical on every platform.
    #[cfg(feature = "rustls-tls")]
    let builder = {
        // rustls needs a process-level crypto provider; install ring exactly
        // like tauri-plugin-updater does, unless the app already chose one.
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        builder.use_preconfigured_tls(tls)
    };

    Ok(builder.build()?)
}

/// GET `url` vào bộ nhớ, từ chối các nội dung vượt quá `cap` byte.
///
/// ---
///
/// GET `url` into memory, refusing bodies over `cap` bytes.
pub(crate) async fn fetch_capped(client: &reqwest::Client, url: &str, cap: u64) -> Result<Vec<u8>> {
    let mut response = checked_get(client, url).await?;
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() as u64 + chunk.len() as u64 > cap {
            return Err(Error::ResponseTooLarge {
                url: url.to_string(),
                limit: cap,
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Truyền phát trực tuyến tệp nén vào `dest`, đồng thời tính mã băm khi tải. Thực thi kiểm tra
/// kích thước `size` chính xác theo manifest đã ký (ngắt kết nối ngay khi luồng tải vượt quá)
/// và kiểm tra mã băm `sha256`. `dest` giữ dữ liệu dở dang khi thất bại — bên gọi chịu trách nhiệm
/// dọn dẹp nó (một tệp tạm tempfile, tự xóa khi drop).
///
/// ---
///
/// Stream the archive into `dest`, hashing as it downloads. Enforces the
/// signed manifest's exact `size` (aborting as soon as the stream exceeds
/// it) and its `sha256`. `dest` holds partial data on failure — the caller
/// owns its cleanup (a tempfile, deleted on drop).
pub(crate) async fn download_archive(
    client: &reqwest::Client,
    archive: &ArchiveInfo,
    dest: &mut File,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    let mut response = checked_get(client, &archive.url).await?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    while let Some(chunk) = response.chunk().await? {
        downloaded += chunk.len() as u64;
        if downloaded > archive.size {
            return Err(Error::ArchiveSize {
                declared: archive.size,
                actual: downloaded,
            });
        }
        hasher.update(&chunk);
        dest.write_all(&chunk)?;
        on_progress(downloaded, archive.size);
    }
    if downloaded != archive.size {
        return Err(Error::ArchiveSize {
            declared: archive.size,
            actual: downloaded,
        });
    }
    let actual = to_hex(&hasher.finalize());
    if actual != archive.sha256 {
        return Err(Error::ArchiveSha256 {
            declared: archive.sha256.clone(),
            actual,
        });
    }
    dest.flush()?;
    Ok(())
}

async fn checked_get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response> {
    let response = client.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(Error::HttpStatus {
            status: status.as_u16(),
            url: url.to_string(),
        });
    }
    Ok(response)
}

/// Chuỗi hex chữ thường — định dạng sha256 chuẩn tắc được sử dụng trong các manifest và danh sách đen báo hỏng.
///
/// ---
///
/// Lowercase hex — the canonical sha256 form used in manifests and the
/// failure blacklist.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}
