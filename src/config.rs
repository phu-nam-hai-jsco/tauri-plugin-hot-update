//! Cấu hình plugin: đối tượng `plugins.hot-update` trong `tauri.conf.json`,
//! được xác thực khi khởi tạo plugin để cấu hình sai sẽ báo lỗi ngay khi khởi động,
//! không bao giờ gây bất ngờ khi đang chạy. Nguồn cập nhật được thiết kế chủ đích chỉ nhận từ cấu hình —
//! mã JS không thể truyền URL manifest hoặc khóa tùy ý, nhờ đó nội dung webview bị xâm phạm cũng không thể chuyển hướng nguồn cập nhật.
//!
//! ---
//!
//! Plugin configuration: the `plugins.hot-update` object in
//! `tauri.conf.json`, validated at plugin init so a bad config is a startup
//! error, never a runtime surprise. The update source is deliberately
//! config-only — JS cannot pass a manifest URL or keys, so compromised
//! webview content can never redirect updates.
//!
//! ```json
//! {
//!   "plugins": {
//!     "hot-update": {
//!       "manifestUrl": "https://updates.example.com/manifest.json",
//!       "pubkeys": ["RWT..."],
//!       "enabled": true
//!     }
//!   }
//! }
//! ```

use serde::Deserialize;

use crate::update::UpdateConfig;
use crate::{Error, Result};

/// `plugins.hot-update` trong `tauri.conf.json`.
///
/// ---
///
/// `plugins.hot-update` in `tauri.conf.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    /// URL của tệp `manifest.json`. Chữ ký tách rời được tải về từ
    /// `{manifestUrl}.minisig`, do đó đây phải là một URL tệp tĩnh đơn thuần không kèm query string.
    /// Bắt buộc khai báo khi `enabled` là true.
    ///
    /// ---
    ///
    /// URL of `manifest.json`. The detached signature is fetched from
    /// `{manifestUrl}.minisig`, so this must be a plain file URL with no
    /// query string. Required when `enabled` is true.
    #[serde(default)]
    pub manifest_url: Option<String>,
    /// Các khóa công khai minisign đáng tin cậy — chuỗi base64 thô (`RW…`) hoặc toàn bộ
    /// nội dung tệp `minisign.pub`. Một manifest xác thực thành công dưới BẤT KỲ khóa nào
    /// đều được tin cậy (khai báo cả khóa cũ + khóa mới khi luân chuyển khóa).
    /// Bắt buộc có ít nhất một khóa khi `enabled` là true.
    ///
    /// ---
    ///
    /// Trusted minisign public keys — raw base64 (`RW…`) or full
    /// `minisign.pub` file contents. A manifest verifying under ANY key is
    /// trusted (ship old + new keys during a rotation). At least one is
    /// required when `enabled` is true.
    #[serde(default)]
    pub pubkeys: Vec<String>,
    /// Công tắc dark-ship: đăng ký plugin nhưng giữ ở trạng thái không hoạt động. Khi là false,
    /// không có bản cập nhật nào được kiểm tra hay phục vụ — ứng dụng chạy trên tài nguyên nhúng
    /// hoàn toàn giống như khi không có plugin.
    ///
    /// ---
    ///
    /// Dark-ship switch: register the plugin but keep it inert. When false,
    /// no update is ever checked or served — the app runs on its embedded
    /// assets exactly as if the plugin were absent.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Config {
    /// Xác thực và chuyển đổi thành [`UpdateConfig`] của pipeline, hoặc trả về `None` khi
    /// bị vô hiệu hóa. Mọi lỗi ở đây đều làm dừng khởi động ứng dụng theo chủ đích thiết kế: cấu hình
    /// nằm trong mã nhị phân gửi lên kho ứng dụng, do đó một mỏ neo tin cậy hoặc URL bị sai định dạng
    /// là lỗi build/cấu hình mà lập trình viên cần nhìn thấy ngay trong lần chạy đầu tiên.
    ///
    /// Cấu hình bị vô hiệu hóa sẽ hoàn toàn không bị kiểm tra xác thực — việc dark-ship với các giá trị
    /// giữ chỗ (placeholder) tuyệt đối không được phép làm hỏng ứng dụng.
    ///
    /// ---
    ///
    /// Validate into the pipeline's [`UpdateConfig`], or `None` when
    /// disabled. Every failure here aborts app startup by design: the config
    /// ships inside the store binary, so a malformed trust anchor or URL is
    /// a build/config bug the developer must see on first run.
    ///
    /// A disabled config is not validated at all — dark-shipping with
    /// placeholder values must never brick the app.
    pub(crate) fn validate(&self) -> Result<Option<UpdateConfig>> {
        if !self.enabled {
            return Ok(None);
        }
        let url = self
            .manifest_url
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if url.is_empty() {
            return Err(Error::Config(
                "`manifestUrl` is required when the plugin is enabled".into(),
            ));
        }
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return Err(Error::Config(format!(
                "`manifestUrl` must be an http(s) URL, got {url:?}"
            )));
        }
        if url.contains('?') {
            return Err(Error::Config(
                "`manifestUrl` must be a plain file URL without a query string \
                 (the detached signature is fetched from `<manifestUrl>.minisig`)"
                    .into(),
            ));
        }
        if self.pubkeys.is_empty() {
            return Err(Error::Config(
                "at least one trusted minisign public key is required in `pubkeys`".into(),
            ));
        }
        crate::manifest::validate_pubkeys(&self.pubkeys)?;
        Ok(Some(UpdateConfig {
            manifest_url: url.to_string(),
            pubkeys: self.pubkeys.clone(),
        }))
    }
}

#[cfg(test)]
mod tests;
