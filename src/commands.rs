//! Bề mặt lệnh IPC — các lớp bọc mỏng trên [`HotUpdate`] có định dạng JSON
//! là hợp đồng giao tiếp liên ngôn ngữ với gói TypeScript (`guest-js/`, npm `tauri-plugin-hot-update-api`).
//! Mọi cấu trúc tuần tự hóa ở đây đều được ghim chặt bằng bài kiểm thử mẫu chuẩn (golden test)
//! trong `commands/tests.rs`; việc đổi tên phía Rust bắt buộc phải báo lỗi kiểm thử trước khi có thể
//! làm hỏng các kiểu dữ liệu TS.
//!
//! Ngữ nghĩa khi bị vô hiệu hóa bởi cấu hình (`plugins.hot-update.enabled: false`):
//! các lệnh dùng để *báo cáo hoặc xác nhận* trạng thái vẫn hoạt động toàn diện — `current_bundle` trả lời
//! trung thực (embedded, phiên bản ứng dụng), `notify_app_ready` và `reset` là các thao tác an toàn không làm gì (no-op),
//! do đó mã khởi động ứng dụng không cần viết thêm điều kiện xử lý đặc biệt khi dark-ship. Các lệnh
//! dùng để *thực hiện cập nhật* (`check`, `download`) sẽ từ chối với lỗi [`Error::Disabled`].
//!
//! ---
//!
//! The IPC command surface — thin wrappers over [`HotUpdate`] whose JSON
//! shapes are the cross-language contract with the TypeScript package
//! (`guest-js/`, npm `tauri-plugin-hot-update-api`). Every serialized shape
//! here is pinned by a golden test in `commands/tests.rs`; a Rust-side
//! rename must fail a test before it can break the TS types.
//!
//! Disabled-by-config semantics (`plugins.hot-update.enabled: false`):
//! commands that *report or ack* state stay total — `current_bundle` answers
//! truthfully (embedded, app version), `notify_app_ready` and `reset` are
//! safe no-ops, so app boot code needs no dark-ship special-casing. Commands
//! that *perform updates* (`check`, `download`) refuse with
//! [`Error::Disabled`].

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::machine::AckOutcome;
use crate::runtime::{BundleSource, CurrentBundle, HotUpdate};
use crate::update::{UpdateConfig, UpdateOutcome};
use crate::{Error, Result};

/// Sự kiện được phát ra trong khi [`download`] truyền phát trực tuyến tệp nén, mang theo
/// dữ liệu [`DownloadProgress`]. Việc phát sự kiện được tiết lưu (throttled) tối đa một sự kiện
/// mỗi 100 ms để cầu nối IPC không bao giờ bị nghẽn bởi các callback từng chunk;
/// chunk cuối cùng (`downloaded == total`) luôn luôn được phát ra.
///
/// ---
///
/// Event emitted while [`download`] streams the archive, carrying a
/// [`DownloadProgress`] payload. Emission is throttled to at most one event
/// per 100 ms so the IPC bridge is never flooded by per-chunk callbacks; the
/// final chunk (`downloaded == total`) is always emitted.
pub const PROGRESS_EVENT: &str = "hot-update://progress";

const PROGRESS_MIN_INTERVAL: Duration = Duration::from_millis(100);

/// Tải trọng dữ liệu của [`PROGRESS_EVENT`]: số byte tệp nén đã nhận được tính đến hiện tại
/// trên tổng số byte trong tệp manifest đã ký.
///
/// ---
///
/// Payload of [`PROGRESS_EVENT`]: archive bytes received so far out of the
/// signed manifest's total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

/// Nguồn cập nhật đã được xác thực, được quản lý cùng với [`HotUpdate`] bởi
/// setup hook của plugin. `None` có nghĩa là bị tắt bởi cấu hình.
///
/// ---
///
/// The validated update source, managed alongside [`HotUpdate`] by the
/// plugin setup hook. `None` means disabled by config.
pub(crate) struct CommandConfig {
    pub(crate) update: Option<UpdateConfig>,
}

impl CommandConfig {
    fn update_config(&self) -> Result<&UpdateConfig> {
        self.update.as_ref().ok_or(Error::Disabled)
    }

    fn is_disabled(&self) -> bool {
        self.update.is_none()
    }
}

/// Cấu trúc truyền tải qua IPC của `notify_app_ready`, ánh xạ tương ứng với [`AckOutcome`].
/// Đây là một kiểu riêng biệt vì enum của máy trạng thái mang tải trọng tuple thô, điều mà serde
/// gắn thẻ nội bộ (internally-tagged) không thể biểu diễn — và vì các cấu trúc dữ liệu IPC thuộc về
/// tầng IPC chứ không thuộc về máy trạng thái thuần túy.
///
/// ---
///
/// Wire shape of `notify_app_ready`, mirroring [`AckOutcome`]. A separate
/// type because the machine's enum carries bare tuple payloads, which
/// internally-tagged serde cannot represent — and because wire shapes belong
/// to the IPC layer, not the pure state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum AckResult {
    Committed { seq: u64 },
    AlreadyCommitted { seq: u64 },
    EmbeddedNoop,
    Stale { seq: u64 },
}

impl From<AckOutcome> for AckResult {
    fn from(outcome: AckOutcome) -> Self {
        match outcome {
            AckOutcome::Committed(seq) => Self::Committed { seq },
            AckOutcome::AlreadyCommitted(seq) => Self::AlreadyCommitted { seq },
            AckOutcome::EmbeddedNoop => Self::EmbeddedNoop,
            AckOutcome::Stale(seq) => Self::Stale { seq },
        }
    }
}

/// Bộ điều tiết tần suất phát tiến độ: callback đầu tiên và bất kỳ sự kiện nào cách lần phát
/// trước đó ít nhất `min_interval` sẽ được thông qua; chunk cuối cùng luôn luôn được thông qua
/// để đảm bảo giao diện người dùng nhìn thấy 100%.
///
/// ---
///
/// Rate limiter for progress emission: the first callback and anything at
/// least `min_interval` after the previous emission pass; the final chunk
/// always passes so the UI is guaranteed to see 100%.
pub(crate) struct ProgressThrottle {
    min_interval: Duration,
    last_emit: Option<Instant>,
}

impl ProgressThrottle {
    pub(crate) fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_emit: None,
        }
    }

    pub(crate) fn should_emit(&mut self, downloaded: u64, total: u64) -> bool {
        let is_final = downloaded >= total;
        let due = self
            .last_emit
            .map_or(true, |at| at.elapsed() >= self.min_interval);
        if is_final || due {
            self.last_emit = Some(Instant::now());
            return true;
        }
        false
    }
}

/// Tải về và xác thực tệp manifest, sau đó báo cáo xem có bản cập nhật nào áp dụng được không.
/// Không bao giờ tự động tải tệp nén. Các trường hợp cổng từ chối (`upToDate`, `blacklisted`,
/// `shellTooOld`, `alreadyStaged`) là các kết quả `Ok`; lỗi chỉ xảy ra khi truyền tải,
/// xác thực chữ ký hoặc cấu hình bị hỏng.
///
/// ---
///
/// Fetch and verify the manifest, then report whether an update applies.
/// Never downloads the archive. Gate refusals (`upToDate`, `blacklisted`,
/// `shellTooOld`, `alreadyStaged`) are `Ok` outcomes; errors are transport,
/// verification, or config failures.
#[tauri::command]
pub(crate) async fn check(
    hot_update: State<'_, HotUpdate>,
    config: State<'_, CommandConfig>,
) -> Result<UpdateOutcome> {
    hot_update.check(config.update_config()?).await
}

/// Toàn bộ quy trình: kiểm tra, và nếu có bản cập nhật phù hợp — tải về, xác thực,
/// giải nén, và lưu vào hàng chờ cho lần khởi động nguội tiếp theo, đồng thời phát ra các sự kiện
/// [`PROGRESS_EVENT`] có tiết lưu nhịp trong quá trình tải. Các lời gọi đồng thời sẽ được tuần tự hóa;
/// lời gọi đến sau sẽ báo `alreadyStaged`/`upToDate` thay vì tải xuống hai lần.
///
/// ---
///
/// The full pipeline: check, and if an update applies — download, verify,
/// extract, and stage it for the next cold launch, emitting throttled
/// [`PROGRESS_EVENT`]s along the way. Concurrent calls are serialized; the
/// loser reports `alreadyStaged`/`upToDate` instead of downloading twice.
#[tauri::command]
pub(crate) async fn download<R: Runtime>(
    app: AppHandle<R>,
    hot_update: State<'_, HotUpdate>,
    config: State<'_, CommandConfig>,
) -> Result<UpdateOutcome> {
    let mut throttle = ProgressThrottle::new(PROGRESS_MIN_INTERVAL);
    hot_update
        .check_and_download(config.update_config()?, |downloaded, total| {
            if throttle.should_emit(downloaded, total) {
                if let Err(e) = app.emit(PROGRESS_EVENT, DownloadProgress { downloaded, total }) {
                    log::warn!("hot-update: failed to emit {PROGRESS_EVENT}: {e}");
                }
            }
        })
        .await
}

/// Xác nhận (commit) gói mà tiến trình này đã khởi động (`notifyAppReady`). Có tính lũy đẳng, và
/// được chủ ý thiết kế an toàn để gọi vô điều kiện trong mỗi lần khởi chạy — bao gồm cả
/// khi đang phục vụ tài nguyên nhúng hoặc khi plugin bị tắt.
///
/// ---
///
/// Commit the bundle this process booted (`notifyAppReady`). Idempotent, and
/// deliberately safe to call unconditionally on every launch — including
/// when serving embedded assets or when the plugin is disabled.
#[tauri::command]
pub(crate) fn notify_app_ready(
    hot_update: State<'_, HotUpdate>,
    config: State<'_, CommandConfig>,
) -> Result<AckResult> {
    if config.is_disabled() {
        return Ok(AckResult::EmbeddedNoop);
    }
    Ok(hot_update.notify_app_ready()?.into())
}

/// Thông tin gói đang được phục vụ tại thời điểm hiện tại. Khi plugin bị tắt, theo định nghĩa,
/// đây là gói tài nguyên nhúng ở chính phiên bản của ứng dụng.
///
/// ---
///
/// What is being served right now. When the plugin is disabled this is, by
/// definition, the embedded bundle at the app's own version.
#[tauri::command]
pub(crate) fn current_bundle<R: Runtime>(
    app: AppHandle<R>,
    hot_update: State<'_, HotUpdate>,
    config: State<'_, CommandConfig>,
) -> Result<CurrentBundle> {
    if config.is_disabled() {
        return Ok(CurrentBundle {
            source: BundleSource::Embedded,
            seq: None,
            version: app.package_info().version.clone(),
        });
    }
    hot_update.current_bundle()
}

/// Lối thoát khẩn cấp để gỡ lỗi/hỗ trợ kỹ thuật: xóa sạch toàn bộ trạng thái OTA cùng các gói bundle
/// và quay về phục vụ bản nhúng ở lần khởi động tiếp theo. Không làm gì (no-op) khi đang tắt (một plugin
/// bị tắt không sở hữu trạng thái sống nào; các phần còn sót lại từ bản build bật trước đó sẽ được
/// xử lý bởi bước phân giải khởi động khi được bật lại).
///
/// ---
///
/// Debug/support escape hatch: wipe all OTA state and bundles and revert to
/// embedded serving on the next launch. A no-op while disabled (a disabled
/// plugin owns no live state; leftovers from a previously enabled build are
/// handled by boot resolution when re-enabled).
#[tauri::command]
pub(crate) fn reset(
    hot_update: State<'_, HotUpdate>,
    config: State<'_, CommandConfig>,
) -> Result<()> {
    if config.is_disabled() {
        return Ok(());
    }
    hot_update.reset()
}

#[cfg(test)]
mod tests;
