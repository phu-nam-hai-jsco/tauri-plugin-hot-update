//! Tệp kê khai cập nhật có chữ ký số: lược đồ, xác thực chữ ký, kiểm tra tính hợp lệ.
//!
//! Thứ tự xác thực là cố định và mang tính cốt lõi về bảo mật (tài liệu thiết kế, mục "Manifest
//! and signing"): chữ ký minisign được xác thực trên **chuỗi byte thô của manifest trước tiên**,
//! đối chiếu với DANH SÁCH khóa tin cậy nhúng sẵn (bất kỳ khóa nào xác thực thành công đều được chấp nhận —
//! đó chính là cơ chế luân chuyển khóa: một bản phát hành qua kho ứng dụng bổ sung khóa mới, cả hai khóa
//! đều có thể ký trong giai đoạn chuyển đổi, khóa cũ sẽ bị loại bỏ sau này). Chỉ sau đó các byte mới được
//! phân tích cú pháp thành JSON và kiểm tra tính hợp lệ. Không có bất kỳ đoạn mã xử lý nào phía sau tiếp xúc
//! với dữ liệu chưa được xác minh.
//!
//! ---
//!
//! The signed update manifest: schema, signature verification, validation.
//!
//! Verification order is fixed and security-relevant (design doc, "Manifest
//! and signing"): the minisign signature is verified over the **raw manifest
//! bytes first**, against the embedded trusted-key LIST (any key may verify —
//! that is how key rotation works: a store release adds the new key, both
//! sign during the transition, the old key is dropped later). Only then are
//! the bytes parsed as JSON and sanity-validated. Nothing downstream ever
//! touches unverified data.

use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Tệp kê khai cập nhật (manifest), chính xác như nội dung được phát hành bên cạnh
/// chữ ký tách rời `.minisig`.
///
/// Các trường không xác định được chủ ý chấp nhận (không dùng `deny_unknown_fields`):
/// các trường mới bổ sung vào lược đồ manifest sau này không được làm hỏng các phiên bản shell cũ —
/// chữ ký đã bảo đảm tính toàn vẹn và đáng tin cậy của toàn bộ tài liệu.
///
/// ---
///
/// The update manifest, exactly as published next to its detached
/// `.minisig` signature.
///
/// Unknown fields are deliberately tolerated (no `deny_unknown_fields`):
/// newer manifest schema additions must not break older installed shells —
/// the signature already guarantees the whole document is trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Phiên bản gói bundle; phải lớn hơn nghiêm ngặt so với con dấu phiên bản của shell.
    /// / Bundle version; must be strictly newer than the shell's watermark.
    pub version: Version,
    /// Dấu thời gian phát hành để thông tin (RFC 3339). Không được dùng trong bất kỳ điều kiện cổng nào —
    /// thứ tự phiên bản mới là yếu tố quyết định — do đó được giữ như một chuỗi mờ (opaque string).
    ///
    /// Informational publish timestamp (RFC 3339). Not used in any gate —
    /// version ordering is what matters — so it stays an opaque string.
    pub created_at: String,
    /// Phiên bản shell (app) tối thiểu có khả năng chạy gói bundle này.
    /// / Minimum shell (app) version able to run this bundle.
    pub min_shell_version: Version,
    pub archive: ArchiveInfo,
}

/// Nơi lưu trữ tệp nén bundle và mã băm tương ứng mà nó phải khớp.
///
/// ---
///
/// Where the bundle archive lives and what it must hash to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInfo {
    pub url: String,
    /// Mã băm hex sha256 của các byte tệp nén. Được chuẩn hóa về chữ thường lúc phân tích
    /// để dùng làm định danh danh sách đen trong `state.json`.
    ///
    /// Hex sha256 of the archive bytes. Canonicalized to lowercase at parse
    /// time so it can serve as the blacklist identity in `state.json`.
    pub sha256: String,
    /// Kích thước byte chính xác của tệp nén; quá trình tải xuống sẽ dừng nếu vượt quá.
    /// / Exact archive byte count; the download aborts past it.
    pub size: u64,
}

/// Xác thực `signature` (nội dung tệp `.minisig`) trên chuỗi byte thô `manifest_bytes`
/// đối chiếu với danh sách khóa tin cậy, sau đó phân tích cú pháp và kiểm tra hợp lệ.
///
/// Bất kỳ một khóa tin cậy đơn lẻ nào xác thực thành công là đủ (luân chuyển khóa). Một khóa bị sai
/// định dạng ở bất kỳ đâu trong danh sách đều báo lỗi nghiêm ngặt [`Error::InvalidPublicKey`] — một mỏ neo
/// tin cậy bị hỏng là lỗi cấu hình cần phải xuất hiện ngay, không được bỏ qua.
///
/// ---
///
/// Verify `signature` (the `.minisig` file contents) over the raw
/// `manifest_bytes` against the trusted key list, then parse and validate.
///
/// Any single trusted key verifying is sufficient (rotation). A malformed
/// key anywhere in the list is a hard [`Error::InvalidPublicKey`] — a broken
/// trust anchor is a config bug that must surface, not be skipped.
pub fn verify_and_parse(
    manifest_bytes: &[u8],
    signature: &str,
    trusted_pubkeys: &[String],
) -> Result<Manifest> {
    if trusted_pubkeys.is_empty() {
        return Err(Error::InvalidPublicKey);
    }
    // Parse every trust anchor before verifying anything: a malformed key
    // must surface even when an earlier key in the list would verify.
    let keys = trusted_pubkeys
        .iter()
        .map(|key| parse_public_key(key))
        .collect::<Result<Vec<_>>>()?;
    let signature = Signature::decode(signature)
        .map_err(|e| Error::ManifestSignature(format!("undecodable .minisig data: {e}")))?;

    let verified = keys
        .iter()
        .any(|key| key.verify(manifest_bytes, &signature, false).is_ok());
    if !verified {
        return Err(Error::ManifestSignature(
            "no trusted public key verified the manifest".into(),
        ));
    }

    let mut manifest: Manifest = serde_json::from_slice(manifest_bytes)?;
    validate(&mut manifest)?;
    Ok(manifest)
}

/// Kiểm tra lúc khởi tạo rằng mọi mỏ neo tin cậy đã cấu hình đều phân tích cú pháp được thành
/// tài liệu khóa minisign ([`verify_and_parse`] sẽ phân tích lại khi sử dụng). Cùng quy tắc dừng ngay:
/// một khóa sai định dạng sẽ đánh trượt toàn bộ danh sách.
///
/// ---
///
/// Init-time check that every configured trust anchor parses as minisign
/// key material ([`verify_and_parse`] re-parses at use). Same hard-stop rule
/// as verification: one malformed key fails the whole list.
pub(crate) fn validate_pubkeys(keys: &[String]) -> Result<()> {
    keys.iter()
        .try_for_each(|key| parse_public_key(key).map(drop))
}

/// Accept either the raw base64 key (`RW…`) or the full two-line
/// `minisign.pub` file contents.
fn parse_public_key(key: &str) -> Result<PublicKey> {
    let key = key.trim();
    let result = if key.lines().count() > 1 {
        PublicKey::decode(key)
    } else {
        PublicKey::from_base64(key)
    };
    result.map_err(|_| Error::InvalidPublicKey)
}

/// Sanity checks on the (already signature-verified) manifest, plus sha256
/// canonicalization to lowercase.
fn validate(manifest: &mut Manifest) -> Result<()> {
    let sha = &manifest.archive.sha256;
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::ManifestInvalid(format!(
            "archive.sha256 {sha:?} is not 64 hex characters"
        )));
    }
    manifest.archive.sha256 = manifest.archive.sha256.to_ascii_lowercase();

    let size = manifest.archive.size;
    if size == 0 || size > crate::extract::MAX_UNCOMPRESSED_BYTES {
        return Err(Error::ManifestInvalid(format!(
            "archive.size {size} is outside 1..={}",
            crate::extract::MAX_UNCOMPRESSED_BYTES
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
