use serde::{Serialize, Serializer};

/// Các lỗi phát sinh từ plugin hot-update.
///
/// Mọi cổng kiểm soát xác minh trong quy trình cập nhật là một điểm dừng tuyệt đối với
/// biến thể riêng của nó — chữ ký, mã băm, hoặc kích thước kiểm tra không hợp lệ sẽ hủy bỏ bản cập nhật;
/// không có đường dẫn nào dạng cảnh-báo-rồi-chạy-tiếp ở bất kỳ đâu trong module này.
///
/// ---
/// Errors surfaced by the hot-update plugin.
///
/// Every verification gate in the update pipeline is a hard stop with its own
/// variant — a failed signature, hash, or size check aborts the update; there
/// is no warn-and-continue path anywhere in this module.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Plugin chưa được khởi tạo: hoặc chưa gọi `install()` trên
    /// context, hoặc `.plugin(init(handle))` chưa được đăng ký, hoặc
    /// quá trình khởi tạo thất bại lúc khởi động (trong trường hợp đó ứng dụng sẽ phục vụ
    /// gói bundle nhúng — mức sàn an toàn dự phòng).
    ///
    /// The plugin was not initialized: either `install()` was not called on
    /// the context, or `.plugin(init(handle))` was not registered, or
    /// initialization failed at boot (in which case the app serves the
    /// embedded bundle — the fail-safe floor).
    #[error("hot-update is not active (plugin not initialized); serving embedded assets")]
    NotActive,

    /// Yêu cầu chuyển vào staged bị từ chối bởi các cổng của máy trạng thái.
    ///
    /// A staging request was refused by the state machine gates.
    #[error("stage refused: {0}")]
    StageRefused(#[from] crate::machine::StageError),

    /// Cấu hình `plugins.hot-update` không hợp lệ. Được đưa ra từ hook
    /// setup của plugin, do đó nó sẽ dừng việc khởi động ứng dụng — cấu hình được tích hợp bên trong tệp thực thi
    /// và cần được phát hiện ngay trong lần chạy đầu tiên của nhà phát triển.
    ///
    /// Invalid `plugins.hot-update` configuration. Raised from the plugin's
    /// setup hook, so it aborts app startup — config ships inside the store
    /// binary and must be caught on the developer's first run.
    #[error("hot-update config invalid: {0}")]
    Config(String),

    /// `check`/`download` được gọi trong khi plugin đã bị vô hiệu hóa bởi
    /// cấu hình (`plugins.hot-update.enabled` là false).
    ///
    /// `check`/`download` was invoked while the plugin is disabled by
    /// config (`plugins.hot-update.enabled` is false).
    #[error("hot-update is disabled by config (`plugins.hot-update.enabled` is false)")]
    Disabled,

    /// Khóa công khai tin cậy đã định cấu hình không phải là dữ liệu khóa minisign hợp lệ.
    /// Đây là một điểm dừng bắt buộc ngay cả khi các khóa khác trong danh sách vẫn xác minh được:
    /// âm thầm bỏ qua một neo tin cậy bị lỗi sẽ làm suy yếu danh sách khóa mà không ai hay biết.
    ///
    /// A configured trusted public key is not valid minisign key material.
    /// This is a hard stop even when other keys in the list would verify:
    /// silently skipping a malformed trust anchor would weaken the key list
    /// without anyone noticing.
    #[error("invalid minisign public key in the trusted key list")]
    InvalidPublicKey,

    /// Chữ ký manifest thất bại: dữ liệu `.minisig` không giải mã được, hoặc không có khóa
    /// nào trong danh sách tin cậy xác minh được các byte của manifest.
    ///
    /// The manifest signature failed: undecodable `.minisig` data, or no key
    /// in the trusted list verified the manifest bytes.
    #[error("manifest signature rejected: {0}")]
    ManifestSignature(String),

    /// Các byte của manifest đã ký không phải là JSON hợp lệ theo schema của manifest.
    ///
    /// The signed manifest bytes are not valid JSON for the manifest schema.
    #[error("manifest is not valid manifest JSON: {0}")]
    ManifestParse(#[from] serde_json::Error),

    /// Manifest phân tích cú pháp được nhưng khai báo các giá trị vô nghĩa (mã sha256
    /// sai định dạng, dung lượng lưu trữ bằng 0 hoặc vượt quá giới hạn).
    ///
    /// The manifest parsed but declares nonsensical values (malformed
    /// sha256, zero or over-cap archive size).
    #[error("manifest invalid: {0}")]
    ManifestInvalid(String),

    /// Lỗi HTTP ở cấp vận chuyển (kết nối, TLS, đọc dữ liệu).
    ///
    /// Transport-level HTTP failure (connect, TLS, read).
    #[error("update request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// Máy chủ cập nhật phản hồi với mã trạng thái không thành công.
    ///
    /// The update server answered with a non-success status.
    #[error("update server returned HTTP {status} for {url}")]
    HttpStatus { status: u16, url: String },

    /// Phần thân phản hồi vượt quá giới hạn an toàn (manifest hoặc chữ ký).
    ///
    /// A response body exceeded its sanity cap (manifest or signature).
    #[error("response for {url} exceeds the {limit}-byte cap")]
    ResponseTooLarge { url: String, limit: u64 },

    /// Số lượng byte của gói nén tải về khác với `archive.size`
    /// trong manifest đã ký (thiếu dữ liệu, hoặc luồng dữ liệu tiếp tục kéo dài).
    ///
    /// The downloaded archive's byte count diverged from the signed
    /// manifest's `archive.size` (short body, or a stream that kept going).
    #[error("archive size mismatch: manifest declares {declared} bytes, got {actual}")]
    ArchiveSize { declared: u64, actual: u64 },

    /// Mã sha256 của gói nén tải về khác với manifest đã ký.
    ///
    /// The downloaded archive's sha256 diverged from the signed manifest.
    #[error("archive sha256 mismatch: manifest declares {declared}, got {actual}")]
    ArchiveSha256 { declared: String, actual: String },

    /// Quá trình giải nén gói bị từ chối (mục độc hại/nguy hiểm) hoặc thất bại.
    ///
    /// Archive extraction was refused (hostile entry) or failed.
    #[error(transparent)]
    Extract(#[from] crate::extract::ExtractError),
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
