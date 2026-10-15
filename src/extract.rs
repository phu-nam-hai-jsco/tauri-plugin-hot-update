//! Quá trình giải nén tar.gz được gia cố an toàn cho các gói bundle tải về.
//!
//! Tệp nén được xem là do kẻ tấn công kiểm soát cho đến khi mã băm sha256 của nó khớp với
//! manifest đã ký — và ngay cả khi đã khớp, cơ chế phòng thủ theo chiều sâu vẫn được áp dụng: bộ giải nén
//! này giả định rằng tệp nén luôn có khả năng chứa mã độc hại. Các biện pháp gia cố:
//!
//! - **Liên kết mềm (Symlink) và liên kết cứng (hardlink) bị từ chối**, không chỉ đơn thuần là bỏ qua.
//!   Nhà cung cấp tài nguyên sử dụng `fs::read` để truy cập file (theo sau các symlink), do đó một mục symlink
//!   có thể làm rò rỉ các tệp trong container (như dữ liệu xác thực) vào webview. Phòng vệ zip-slip đơn thuần
//!   là không đủ.
//! - Chỉ các tệp thông thường và thư mục mới được giải nén; mọi loại mục khác (thiết bị, FIFO, tệp thưa sparse,
//!   các phần mở rộng không xác định) đều là lỗi dừng ngay lập tức.
//! - Đường dẫn các mục phải là đường dẫn chuẩn hóa tương đối nghiêm ngặt: không có đường dẫn tuyệt đối,
//!   không có thành phần `..` hoặc `.`, không có tiền tố Windows. Mọi dạng khác đều bị từ chối trước khi
//!   bất kỳ thao tác I/O nào diễn ra cho mục đó.
//! - Giới hạn phòng chống zip-bomb: giới hạn số lượng tệp và giới hạn tổng số byte sau giải nén
//!   (được kiểm tra đối chiếu dung lượng khai báo của từng mục *trước khi* đọc dữ liệu), kèm theo giới hạn
//!   byte thô kéo qua bộ giải mã gzip — để cả siêu dữ liệu tar mà crate `tar` đệm nội bộ (ví dụ GNU long names)
//!   cũng không thể phình to vô hạn.
//! - Không bảo lưu các thuộc tính đặc biệt: các tệp được lưu với quyền mặc định, quyền sở hữu và mtime mặc định.
//!   Tài nguyên web không cần đến những thuộc tính này.
//! - Mỗi tệp được giải nén đều được chạy fsync. Một gói bundle bị rách nát sau khi mất điện đột ngột sẽ khiến
//!   lần khởi động thử nghiệm thất bại và đưa vĩnh viễn một mã băm tệp nén *tốt* vào danh sách đen; tính bền vững
//!   ở bước này chính là yếu tố giữ cho bản cập nhật đó tiếp tục phân phối thành công.
//!
//! ---
//!
//! Hardened tar.gz extraction for downloaded bundles.
//!
//! The archive is attacker-controlled until its sha256 matches the signed
//! manifest — and even then, defense in depth: this extractor assumes the
//! archive is hostile. Hardening (WP2 handoff + design doc):
//!
//! - **Symlinks and hardlinks are rejected**, not skipped. The assets
//!   provider's `fs::read` follows symlinks, so a symlinked entry could
//!   exfiltrate container files (auth data) into the webview. Zip-slip
//!   guarding alone is insufficient.
//! - Only plain files and directories are extracted; every other entry type
//!   (devices, FIFOs, sparse files, unknown extensions) is a hard error.
//! - Entry paths must be strictly relative-normal: no absolute paths, no
//!   `..` or `.` components, no Windows prefixes. Anything else is refused
//!   before any I/O happens for that entry.
//! - Zip-bomb caps: a file-count cap and a total-uncompressed-bytes cap
//!   (checked against each entry's declared size *before* reading its data),
//!   plus a raw cap on bytes pulled through the gzip decoder — so even tar
//!   metadata the `tar` crate buffers internally (e.g. GNU long names)
//!   cannot expand without bound.
//! - Nothing exotic is preserved: files land with default permissions,
//!   ownership, and mtimes. Web assets need none of it.
//! - Each extracted file is fsync'd. A torn bundle after a power loss would
//!   otherwise fail its trial boot and permanently blacklist a *good*
//!   archive hash; durability here is what keeps that update deliverable.

use std::cell::Cell;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use flate2::read::GzDecoder;

/// Số lượng mục tối đa (tệp + thư mục) trong một tệp nén bundle.
/// Bản phân phối web thông thường có dưới một nghìn mục; mức 10.000 tạo không gian rộng rãi
/// trong khi vẫn ngăn chặn việc lạm dụng inode/dirent.
///
/// ---
///
/// Maximum number of entries (files + directories) in a bundle archive.
/// A web dist is typically well under a thousand entries; 10k leaves room
/// while bounding inode/dirent abuse.
pub const MAX_FILES: usize = 10_000;

/// Tổng số byte payload chưa nén tối đa trên tất cả các mục. Các bundle là bản dist frontend
/// (vài MB khi nén); mức 500 MB chặn đứng các quả bom giải nén (decompression bomb) mà không bao giờ
/// hạn chế một bản phát hành hợp lệ.
///
/// ---
///
/// Maximum total uncompressed payload bytes across all entries. Bundles are
/// frontend dists (a few MB compressed); 500 MB bounds decompression bombs
/// without ever constraining a legitimate release.
pub const MAX_UNCOMPRESSED_BYTES: u64 = 500 * 1024 * 1024;

/// Khoảng dung sai bổ sung trên [`MAX_UNCOMPRESSED_BYTES`] cho giới hạn luồng giải nén thô:
/// chi phí đóng khung tar (tiêu đề 512-byte + padding cho mỗi mục, siêu dữ liệu tên dài).
/// 32 MB đủ bao phủ vượt mức cho [`MAX_FILES`] mục.
///
/// ---
///
/// Slack on top of [`MAX_UNCOMPRESSED_BYTES`] for the raw decompressed
/// stream cap: tar framing overhead (512-byte headers + padding per entry,
/// long-name metadata). 32 MB covers [`MAX_FILES`] entries several times
/// over.
const STREAM_SLACK_BYTES: u64 = 32 * 1024 * 1024;

/// Lý do việc giải nén bị từ chối hoặc thất bại. Mỗi biến thể đều là một điểm dừng nghiêm ngặt;
/// bên gọi sẽ hủy bỏ thư mục đầu ra dở dang.
///
/// ---
///
/// Why extraction was refused or failed. Every variant is a hard stop; the
/// caller discards the partial output directory.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("archive read/write failed: {0}")]
    Io(#[from] io::Error),
    #[error("archive entry {path:?} has forbidden type {entry_type} (only plain files and directories are allowed)")]
    ForbiddenEntryType {
        path: String,
        entry_type: &'static str,
    },
    #[error("archive entry path {path:?} is unsafe (absolute, traversing, or non-normal)")]
    UnsafePath { path: String },
    #[error("archive exceeds the {limit}-entry cap")]
    TooManyEntries { limit: usize },
    #[error("archive exceeds the {limit}-byte uncompressed cap")]
    TooLarge { limit: u64 },
}

/// Giải nén `archive` (tar.gz) vào thư mục `target` đã tồn tại,
/// thực thi các giới hạn trong môi trường thực tế ([`MAX_FILES`], [`MAX_UNCOMPRESSED_BYTES`]).
///
/// ---
///
/// Extract `archive` (tar.gz) into the existing directory `target`,
/// enforcing the production caps ([`MAX_FILES`], [`MAX_UNCOMPRESSED_BYTES`]).
pub(crate) fn extract_tar_gz(archive: &Path, target: &Path) -> Result<(), ExtractError> {
    let limits = Limits {
        max_entries: MAX_FILES,
        max_total_bytes: MAX_UNCOMPRESSED_BYTES,
    };
    extract_with_limits(archive, target, limits)
}

/// Các giới hạn, được tách riêng khỏi các hằng số để các bài kiểm thử có thể thực thi logic kiểm tra
/// với các giá trị nhỏ. Môi trường thực tế luôn luôn sử dụng các hằng số được tài liệu hóa.
///
/// ---
///
/// Caps, separated from the constants so tests can exercise the enforcement
/// logic with small values. Production always uses the documented constants.
#[derive(Debug, Clone, Copy)]
struct Limits {
    max_entries: usize,
    max_total_bytes: u64,
}

fn extract_with_limits(archive: &Path, target: &Path, limits: Limits) -> Result<(), ExtractError> {
    let stream_cap = limits.max_total_bytes.saturating_add(STREAM_SLACK_BYTES);
    let exceeded = Rc::new(Cell::new(false));
    let reader = CappedReader {
        inner: GzDecoder::new(fs::File::open(archive)?),
        remaining: stream_cap,
        exceeded: Rc::clone(&exceeded),
    };
    let mut tar = tar::Archive::new(reader);

    let mut entry_count: usize = 0;
    let mut total_bytes: u64 = 0;
    let mut entries = tar.entries()?;
    loop {
        let entry = match entries.next() {
            Some(Ok(entry)) => entry,
            Some(Err(e)) => return Err(map_stream_error(e, &exceeded, limits)),
            None => break,
        };

        entry_count += 1;
        if entry_count > limits.max_entries {
            return Err(ExtractError::TooManyEntries {
                limit: limits.max_entries,
            });
        }

        let raw_path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let entry_type = entry.header().entry_type();
        let kind = match entry_type {
            tar::EntryType::Regular => EntryKind::File,
            tar::EntryType::Directory => EntryKind::Dir,
            other => {
                return Err(ExtractError::ForbiddenEntryType {
                    path: raw_path,
                    entry_type: describe_entry_type(other),
                })
            }
        };

        let dest = safe_join(target, &raw_path)?;
        match kind {
            EntryKind::Dir => fs::create_dir_all(&dest)?,
            EntryKind::File => {
                // Budget from the declared size BEFORE reading any data.
                total_bytes = total_bytes.saturating_add(entry.header().size()?);
                if total_bytes > limits.max_total_bytes {
                    return Err(ExtractError::TooLarge {
                        limit: limits.max_total_bytes,
                    });
                }
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut entry = entry;
                let mut file = fs::File::create(&dest)?;
                match io::copy(&mut entry, &mut file) {
                    Ok(_) => {}
                    Err(e) => return Err(map_stream_error(e, &exceeded, limits)),
                }
                file.sync_all()?;
            }
        }
    }
    Ok(())
}

enum EntryKind {
    File,
    Dir,
}

/// A stream-cap hit surfaces from the tar crate as a wrapped I/O error;
/// translate it back into the typed cap error.
fn map_stream_error(e: io::Error, exceeded: &Cell<bool>, limits: Limits) -> ExtractError {
    if exceeded.get() {
        ExtractError::TooLarge {
            limit: limits.max_total_bytes,
        }
    } else {
        ExtractError::Io(e)
    }
}

fn describe_entry_type(t: tar::EntryType) -> &'static str {
    use tar::EntryType::*;
    match t {
        Symlink => "symlink",
        Link => "hardlink",
        Char => "char device",
        Block => "block device",
        Fifo => "fifo",
        Continuous => "contiguous file",
        GNUSparse => "sparse file",
        XGlobalHeader => "pax global header",
        XHeader => "pax header",
        _ => "unsupported",
    }
}

/// Join an entry path onto the target dir, refusing anything that is not a
/// plain chain of normal components. Same discipline as the serving layer's
/// `safe_join` — but here the input is attacker-controlled bytes, not a
/// tauri-normalized `AssetKey`, so absolute paths and `..`/`.` components
/// are live threats, not belt-and-braces.
fn safe_join(target: &Path, raw: &str) -> Result<PathBuf, ExtractError> {
    let unsafe_path = || ExtractError::UnsafePath { path: raw.into() };
    // Windows-style separators and drive letters never appear in honest
    // unix-built archives; reject rather than interpret.
    if raw.is_empty() || raw.contains('\\') || raw.contains(':') {
        return Err(unsafe_path());
    }
    let mut out = target.to_path_buf();
    let mut pushed = false;
    for component in Path::new(raw).components() {
        match component {
            Component::Normal(part) => {
                out.push(part);
                pushed = true;
            }
            _ => return Err(unsafe_path()),
        }
    }
    if !pushed {
        return Err(unsafe_path());
    }
    Ok(out)
}

/// Hard cap on bytes read through the gzip decoder. Flips `exceeded` and
/// errors once the cap is crossed, no matter what the tar crate is buffering.
struct CappedReader<R: Read> {
    inner: R,
    remaining: u64,
    exceeded: Rc<Cell<bool>>,
}

impl<R: Read> Read for CappedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            self.exceeded.set(true);
            return Err(io::Error::other("decompressed stream exceeds the size cap"));
        }
        let allowed = usize::try_from(self.remaining.min(buf.len() as u64)).unwrap_or(buf.len());
        let n = self.inner.read(&mut buf[..allowed])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}

#[cfg(test)]
mod tests;
