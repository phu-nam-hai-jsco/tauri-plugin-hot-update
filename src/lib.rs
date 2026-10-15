//! Cập nhật nóng (Hot update) / Cập nhật trực tiếp qua mạng OTA cho ứng dụng di động và desktop chạy Tauri v2 —
//! phong cách CodePush, tự lưu trữ (self-hosted).
//!
//! Phục vụ frontend từ một gói OTA đã tải về khi có bản cập nhật đang hoạt động,
//! tự động dự phòng về tài nguyên nhúng được biên dịch từ `frontendDist`. Các bản cập nhật
//! chỉ được áp dụng vào lần khởi động nguội (cold launch) tiếp theo, được bảo vệ bởi con trỏ
//! hoàn tác ba trạng thái (`staged → booting → committed`): gói nào không bao giờ gửi xác nhận qua
//! [`HotUpdate::notify_app_ready`] sẽ bị đưa vào danh sách đen theo mã băm tệp nén ở lần khởi động
//! kế tiếp, và việc phục vụ tài nguyên sẽ quay về gói hoạt động tốt gần nhất hoặc tài nguyên nhúng.
//! Một gói OTA lỗi không bao giờ có thể làm hỏng (brick) ứng dụng đã cài đặt.
//!
//! # Tích hợp (hai bước)
//!
//! Việc hoán đổi tài nguyên phải diễn ra trên `Context` trước khi `Builder::build`
//! tiêu thụ nó, trong khi việc phân giải đường dẫn (thư mục dữ liệu ứng dụng) chỉ khả dụng
//! khi ứng dụng đang được dựng — do đó chia làm hai bước dùng chung một handle:
//!
//! ```rust,ignore
//! fn main() {
//!     let mut context = tauri::generate_context!();
//!     // 1. Hoán đổi tài nguyên nhúng sang nhà cung cấp hot-update.
//!     let hot_update = tauri_plugin_hot_update::install(&mut context);
//!     tauri::Builder::default()
//!         // 2. Đăng ký plugin; setup hook của nó sẽ phân giải kho lưu trữ
//!         //    bundle và kích hoạt/hoàn tác TRƯỚC KHI bất kỳ webview nào tồn tại.
//!         .plugin(tauri_plugin_hot_update::init(hot_update))
//!         .run(context)
//!         .expect("error while running tauri application");
//! }
//! ```
//!
//! Cấu hình nằm trong `tauri.conf.json` (xem [`Config`]) và được xác thực bên trong
//! setup hook — URL manifest hoặc khóa tin cậy bị sai định dạng sẽ dừng khởi động ngay
//! trên máy lập trình viên, tuyệt đối không bị lỗi âm thầm khi ra môi trường thực tế.
//! Năm lệnh IPC (`check`, `download`, `notify_app_ready`, `current_bundle`, `reset`;
//! gói npm `tauri-plugin-hot-update-api`) phải được cấp quyền trong tệp capability, ví dụ:
//! `"permissions": ["hot-update:default"]`.
//!
//! Hãy đăng ký plugin này trước các plugin khác để không thành phần nào truy cập tài nguyên
//! sớm hơn bước phân giải khởi động. Frontend phải gọi `notifyAppReady()` sau khi khung
//! ứng dụng (app shell) đã mount và render xong — chủ đích độc lập hoàn toàn với kết nối mạng
//! hoặc xác thực, giúp sự cố backend không bao giờ làm hỏng hàng loạt gói bundle tốt trên toàn bộ thiết bị.
//!
//! # Bảo đảm thứ tự thực thi
//!
//! Các setup hook của plugin chạy bên trong `Builder::build` (tauri 2.10.2
//! `app.rs:2289`), hoàn toàn diễn ra trước khi các cửa sổ và webview được tạo
//! (`app.rs:2373`). Do đó, việc nâng trạng thái staged→booting được lưu bền vững xuống đĩa
//! trước khi nhà cung cấp phục vụ dù chỉ một byte, trên mọi nền tảng — bao gồm cả
//! Android, nơi thư mục dữ liệu ứng dụng không thể phân giải trước khi ứng dụng tồn tại.
//!
//! ---
//!
//! Hot update / OTA live updates for Tauri v2 mobile and desktop apps —
//! CodePush-style, self-hosted.
//!
//! Serves the frontend from a downloaded OTA bundle when one is active,
//! falling back to the embedded assets compiled from `frontendDist`. Updates
//! apply on the next cold launch only, guarded by a three-state rollback
//! pointer (`staged → booting → committed`): a bundle that never acks via
//! [`HotUpdate::notify_app_ready`] is blacklisted by archive hash on the next
//! boot and serving falls back to the last-known-good bundle or the embedded
//! assets. One bad OTA push can never brick an installed app.
//!
//! # Integration (two steps)
//!
//! The assets swap must happen on the `Context` before `Builder::build`
//! consumes it, while path resolution (the app data dir) is only available
//! once the app is being built — hence two steps sharing one handle:
//!
//! ```rust,ignore
//! fn main() {
//!     let mut context = tauri::generate_context!();
//!     // 1. Swap the embedded assets for the hot-update provider.
//!     let hot_update = tauri_plugin_hot_update::install(&mut context);
//!     tauri::Builder::default()
//!         // 2. Register the plugin; its setup hook resolves the bundle
//!         //    store and arms/rolls back BEFORE any webview exists.
//!         .plugin(tauri_plugin_hot_update::init(hot_update))
//!         .run(context)
//!         .expect("error while running tauri application");
//! }
//! ```
//!
//! Configuration lives in `tauri.conf.json` (see [`Config`]) and is
//! validated inside the setup hook — a malformed manifest URL or trust
//! anchor aborts startup on the developer's machine, never silently in the
//! field. The five IPC commands (`check`, `download`, `notify_app_ready`,
//! `current_bundle`, `reset`; npm package `tauri-plugin-hot-update-api`)
//! must be granted in a capability file, e.g. `"permissions":
//! ["hot-update:default"]`.
//!
//! Register this plugin before other plugins so nothing observes assets
//! earlier than the boot resolution. The frontend must call
//! `notifyAppReady()` once the app shell has mounted and rendered —
//! deliberately independent of network reachability or auth, so a backend
//! outage can never condemn a good bundle fleet-wide.
//!
//! # Ordering guarantee
//!
//! Plugin setup hooks run inside `Builder::build` (tauri 2.10.2
//! `app.rs:2289`), strictly before config windows and webviews are created
//! (`app.rs:2373`). The staged→booting promotion is therefore persisted
//! before the provider serves a single byte, on every platform — including
//! Android, where the app data dir cannot be resolved before the app exists.

use std::sync::Arc;

use tauri::plugin::{Builder as PluginBuilder, TauriPlugin};
use tauri::{Manager, Runtime};

mod assets;
mod commands;
mod config;
mod download;
mod error;
mod extract;
mod machine;
mod manifest;
mod runtime;
/// Ký số phía phát hành (phần lõi của CLI `hot-update-sign`). Chỉ công khai khi bật
/// feature `cli`; đồng thời được biên dịch cho các bài kiểm thử để bộ test đối soát
/// toàn diện giữa mã ký thực tế và quy trình xác thực.
///
/// ---
///
/// Release-side signing (the `hot-update-sign` CLI's core). Public only with
/// the `cli` feature; also compiled for tests so the suite round-trips the
/// real signing code against the verify path.
#[cfg(any(feature = "cli", test))]
pub mod sign;
mod store;
#[cfg(test)]
mod testutil;
mod update;

pub use assets::HotUpdateAssets;
pub use commands::{DownloadProgress, PROGRESS_EVENT};
pub use config::Config;
pub use error::{Error, Result};
pub use extract::ExtractError;
pub use machine::{AckOutcome, StageError};
pub use manifest::{ArchiveInfo, Manifest};
pub use runtime::{BundleSource, CurrentBundle, HotUpdate};
pub use update::{UpdateConfig, UpdateOutcome};

use runtime::Shared;

/// Liên kết ẩn giữa [`install`] (tạo nhà cung cấp tài nguyên) và
/// [`init`] (kích hoạt nhà cung cấp khi đường dẫn đã phân giải được).
///
/// ---
///
/// Opaque link between [`install`] (which creates the assets provider) and
/// [`init`] (which activates it once paths are resolvable).
pub struct HotUpdateHandle {
    shared: Arc<Shared>,
}

/// Bước 1: Hoán đổi tài nguyên nhúng của context sang nhà cung cấp hot-update.
/// Phải được gọi trước khi `tauri::Builder::build`/`run` tiêu thụ context.
/// Truyền handle trả về vào [`init`].
///
/// Cho đến khi setup hook của [`init`] chạy, nhà cung cấp sẽ phục vụ tài nguyên nhúng
/// — chốt chặn an toàn dự phòng.
///
/// ---
///
/// Step 1: swap the generated context's embedded assets for the hot-update
/// provider. Must be called before `tauri::Builder::build`/`run` consumes
/// the context. Pass the returned handle to [`init`].
///
/// Until [`init`]'s setup hook runs, the provider serves the embedded assets
/// — the fail-safe floor.
pub fn install<R: Runtime>(context: &mut tauri::Context<R>) -> HotUpdateHandle {
    let shared = Arc::new(Shared::default());
    // Two-step swap through a placeholder: the wrapper must own the original
    // embedded box before it can be constructed.
    let embedded = context.set_assets(Box::new(assets::PlaceholderAssets));
    context.set_assets(Box::new(HotUpdateAssets::new(
        embedded,
        Arc::clone(&shared),
    )));
    HotUpdateHandle { shared }
}

/// Bước 2: Khởi tạo plugin. Setup hook của nó (được tauri chạy trước khi bất kỳ webview nào
/// được tạo) xác thực [`Config`] từ `tauri.conf.json`, phân giải `{app_data_dir}/hot-update`,
/// tải `state.json`, thực hiện hoàn tác/kích hoạt, lưu bền vững và bắt đầu phục vụ.
///
/// Cấu hình `plugins.hot-update` bị thiếu hoặc không hợp lệ sẽ dừng quá trình khởi động (cấu hình
/// là cấu phần lúc build — đặt `{ "enabled": false }` nếu muốn tắt thử nghiệm).
/// Các sự cố *khi đang chạy* (không phân giải được thư mục dữ liệu, không ghi được trạng thái)
/// sẽ tự động hạ cấp về phục vụ tài nguyên nhúng; không bao giờ làm tắt ứng dụng.
///
/// ---
///
/// Step 2: the plugin. Its setup hook (which tauri runs before any webview
/// is created) validates the [`Config`] from `tauri.conf.json`, resolves
/// `{app_data_dir}/hot-update`, loads `state.json`, performs
/// rollback/arming, persists, and activates serving.
///
/// A missing or invalid `plugins.hot-update` config aborts startup (config
/// is a build-time artifact — set `{ "enabled": false }` to dark-ship).
/// *Runtime* failures (unresolvable data dir, unwritable state) degrade to
/// serving embedded assets; they never abort the app.
pub fn init<R: Runtime>(handle: HotUpdateHandle) -> TauriPlugin<R, Option<Config>> {
    init_with_root(handle, None)
}

/// Biến thể chỉ dùng trong kiểm thử, ghim thư mục gốc của kho lưu trữ vào thư mục tạm thay vì
/// gọi `app_data_dir()` thực tế (trên mock runtime sẽ phân giải vào thư mục home người dùng thật).
///
/// ---
///
/// Test-only variant pinning the store root to a temp dir instead of the
/// real `app_data_dir()` (which on a mock runtime resolves into the real
/// user home).
#[cfg(test)]
pub(crate) fn init_for_test<R: Runtime>(
    handle: HotUpdateHandle,
    root: std::path::PathBuf,
) -> TauriPlugin<R, Option<Config>> {
    init_with_root(handle, Some(root))
}

fn init_with_root<R: Runtime>(
    handle: HotUpdateHandle,
    root_override: Option<std::path::PathBuf>,
) -> TauriPlugin<R, Option<Config>> {
    PluginBuilder::<R, Option<Config>>::new("hot-update")
        .invoke_handler(tauri::generate_handler![
            commands::check,
            commands::download,
            commands::notify_app_ready,
            commands::current_bundle,
            commands::reset,
        ])
        .setup(move |app, api| {
            let config = api.config().as_ref().ok_or_else(|| {
                Error::Config(
                    "missing `plugins.hot-update` in tauri.conf.json; \
                     set { \"enabled\": false } to ship the plugin dark"
                        .into(),
                )
            })?;
            let update = config.validate()?;
            if update.is_some() {
                let root = match &root_override {
                    Some(root) => Some(root.clone()),
                    None => match app.path().app_data_dir() {
                        Ok(base) => Some(base.join("hot-update")),
                        Err(e) => {
                            log::error!(
                                "hot-update: app_data_dir() failed ({e}); \
                                 serving embedded assets only"
                            );
                            None
                        }
                    },
                };
                if let Some(root) = root {
                    let embedded_version = app.package_info().version.clone();
                    runtime::initialize(&handle.shared, root, embedded_version);
                }
            } else {
                log::info!("hot-update: disabled by config; serving embedded assets");
            }
            app.manage(HotUpdate {
                shared: Arc::clone(&handle.shared),
            });
            app.manage(commands::CommandConfig { update });
            Ok(())
        })
        .build()
}

/// Truy cập Runtime API [`HotUpdate`] từ bất kỳ `Manager` nào (app handle,
/// window, webview).
///
/// Sẽ panic nếu plugin chưa được đăng ký qua [`init`].
///
/// ---
///
/// Access the [`HotUpdate`] runtime API from any `Manager` (app handle,
/// window, webview).
///
/// Panics if the plugin was not registered via [`init`].
pub trait HotUpdateExt<R: Runtime> {
    fn hot_update(&self) -> &HotUpdate;
}

impl<R: Runtime, T: Manager<R>> HotUpdateExt<R> for T {
    fn hot_update(&self) -> &HotUpdate {
        self.state::<HotUpdate>().inner()
    }
}
