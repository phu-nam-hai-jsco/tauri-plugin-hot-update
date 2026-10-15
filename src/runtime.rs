//! Kết nối lúc khởi động giữa máy trạng thái thuần túy và lớp vỏ I/O, cùng với
//! API runtime trong tiến trình ([`HotUpdate`]).
//!
//! Đảm bảo về thứ tự thực thi (bất biến chống treo/hỏng): [`initialize`] chạy bên trong
//! setup hook của plugin, vốn được tauri thực thi trong `Builder::build`
//! (tauri 2.10.2 `app.rs:2289`; đã xác minh lại trên 2.11.5 `app.rs:2440`) —
//! hoàn toàn trước khi bất kỳ cửa sổ hay webview nào được tạo ra, và do đó trước khi
//! nhà cung cấp assets phục vụ dù chỉ một byte. Việc thăng cấp staged→booting
//! được lưu bền vững bên trong [`initialize`], vì vậy khi việc phục vụ bắt đầu thì dấu hiệu rollback
//! đã nằm sẵn trên đĩa. Nếu việc lưu thất bại, gói vừa được trang bị sẽ *không*
//! được phục vụ (lần khởi động thử mà không có dấu hiệu lưu bền vững có thể lọt qua cơ chế rollback).
//!
//! ---
//! Boot-time wiring between the pure state machine and the I/O shell, plus
//! the in-process runtime API ([`HotUpdate`]).
//!
//! Ordering guarantee (the anti-brick invariant): [`initialize`] runs inside
//! the plugin's setup hook, which tauri executes during `Builder::build`
//! (tauri 2.10.2 `app.rs:2289`; re-verified on 2.11.5 `app.rs:2440`) —
//! strictly before any window or webview is created, and therefore before
//! the assets provider serves a single byte. The staged→booting promotion
//! is persisted inside [`initialize`], so by the time serving starts the rollback marker is
//! already on disk. If persisting fails, the newly armed bundle is *not*
//! served (a trial boot without a persisted marker could evade rollback).

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use semver::Version;
use serde::Serialize;

use crate::machine::{self, AckOutcome, Active};
use crate::store::Store;
use crate::{Error, Result};

/// Cell shared between the assets provider (created at `install()` time,
/// before the app exists) and the plugin (which resolves paths and runs boot
/// resolution once the app is being built). Written exactly once.
#[derive(Default)]
pub(crate) struct Shared {
    activation: OnceLock<Activation>,
}

/// Everything fixed for the lifetime of the process at boot resolution.
pub(crate) struct Activation {
    store: Store,
    /// The source this process serves. Never changes mid-session — a
    /// download that finishes later only stages for the *next* boot.
    active: Active,
    /// Bundle dir for [`Active::Ota`], resolved once.
    active_dir: Option<PathBuf>,
    /// Version of the active bundle (OTA) or of the embedded assets.
    active_version: Version,
    embedded_version: Version,
    /// Serializes update pipeline runs ([`crate::update`]): concurrent
    /// check/download calls must not race seq allocation or double-download.
    update_lock: tokio::sync::Mutex<()>,
}

impl Activation {
    pub(crate) fn store(&self) -> &Store {
        &self.store
    }

    /// The shell version the compatibility gate checks `minShellVersion`
    /// against. `version:bump` keeps the embedded frontend in lockstep with
    /// the app version, so this is also the embedded bundle version.
    pub(crate) fn embedded_version(&self) -> &Version {
        &self.embedded_version
    }

    pub(crate) fn update_lock(&self) -> &tokio::sync::Mutex<()> {
        &self.update_lock
    }
}

impl Shared {
    /// Directory the provider should serve from, or `None` for embedded.
    /// `None` before activation: the fail-safe default is embedded.
    pub(crate) fn active_dir(&self) -> Option<&PathBuf> {
        self.activation.get()?.active_dir.as_ref()
    }

    pub(crate) fn is_activated(&self) -> bool {
        self.activation.get().is_some()
    }

    pub(crate) fn activation(&self) -> Result<&Activation> {
        self.activation.get().ok_or(Error::NotActive)
    }
}

/// Run boot resolution against `root` (the `hot-update/` dir) and activate
/// serving. Called from the plugin setup hook; must never panic or fail the
/// app — every failure path degrades to serving embedded assets.
pub(crate) fn initialize(shared: &Shared, root: PathBuf, embedded_version: Version) {
    let store = Store::new(root);
    let state = store.load_state();
    let present = store.present_seqs();
    let outcome = machine::resolve_boot(state, &embedded_version, &present);

    if let Some(seq) = outcome.rolled_back {
        log::warn!(
            "hot-update: bundle seq {seq} was armed last boot but never acked; \
             rolled back and blacklisted its archive hash"
        );
    }

    // Persist BEFORE serving. On failure, fall back to what the previous
    // persisted state already supports: the committed bundle survived boot
    // validation and its pointer is already durable, but a *newly armed*
    // bundle must not run without its rollback marker on disk.
    let active = match store.save_state(&outcome.state) {
        Ok(()) => {
            store.apply_effects(&outcome.effects);
            store.sweep_foreign_entries();
            outcome.active
        }
        Err(e) => {
            log::error!(
                "hot-update: failed to persist state.json ({e}); \
                 serving committed/embedded without arming the staged bundle"
            );
            match outcome.state.committed {
                Some(seq) => Active::Ota(seq),
                None => Active::Embedded,
            }
        }
    };

    let (active_dir, active_version) = match active {
        Active::Ota(seq) => {
            // resolve_boot only arms/keeps seqs it has metadata for.
            let version = outcome
                .state
                .versions
                .get(&seq)
                .map(|meta| meta.version.clone())
                .unwrap_or_else(|| embedded_version.clone());
            (Some(store.bundle_dir(seq)), version)
        }
        Active::Embedded => (None, embedded_version.clone()),
    };

    match active {
        Active::Ota(seq) => log::info!(
            "hot-update: serving OTA bundle seq {seq} (v{active_version}) from {}",
            store.bundle_dir(seq).display()
        ),
        Active::Embedded => {
            log::info!("hot-update: serving embedded assets (v{embedded_version})")
        }
    }

    let activation = Activation {
        store,
        active,
        active_dir,
        active_version,
        embedded_version,
        update_lock: tokio::sync::Mutex::new(()),
    };
    if shared.activation.set(activation).is_err() {
        log::warn!("hot-update: initialize called twice; keeping the first activation");
    }
}

/// Nơi cung cấp frontend hiện đang được phục vụ.
///
/// Where the currently served frontend comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BundleSource {
    Embedded,
    Ota,
}

/// Ảnh chụp nhanh (snapshot) về những gì tiến trình này đang phục vụ.
///
/// Snapshot of what this process is serving.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentBundle {
    pub source: BundleSource,
    /// Seq của gói khi `source == Ota`.
    ///
    /// Bundle seq when `source == Ota`.
    pub seq: Option<u64>,
    pub version: Version,
}

/// API Runtime, được quản lý trong trạng thái ứng dụng bởi [`crate::init`]. Các lệnh IPC
/// (WP4) sẽ là các lớp bọc mỏng bên ngoài những phương thức này.
///
/// Runtime API, managed in the app state by [`crate::init`]. IPC commands
/// (WP4) will be thin wrappers over these methods.
pub struct HotUpdate {
    pub(crate) shared: Arc<Shared>,
}

impl HotUpdate {
    /// Xác nhận commit gói mà tiến trình này đã khởi động (`notifyAppReady`).
    ///
    /// Commit seq đã khởi động trong bộ nhớ được ghi lại khi phân giải khởi động — không bao giờ
    /// đọc lại giá trị từ đĩa — để một tiến trình tải xuống hoàn tất giữa phiên
    /// vẫn ở trong trạng thái staged chờ lần khởi động thử nghiệm riêng của nó. Có tính lũy đẳng.
    ///
    /// ---
    /// Commit the bundle this process booted (`notifyAppReady`).
    ///
    /// Commits the in-memory booted seq captured at boot resolution — never
    /// a value re-read from disk — so a download that finished mid-session
    /// stays staged for its own trial boot. Idempotent.
    pub fn notify_app_ready(&self) -> Result<AckOutcome> {
        let activation = self.shared.activation()?;
        let (outcome, effects) = activation.store.update(|state| {
            let (state, outcome, effects) = machine::ack(state, activation.active);
            (state, (outcome, effects))
        })?;
        activation.store.apply_effects(&effects);
        match outcome {
            AckOutcome::Committed(seq) => {
                log::info!("hot-update: bundle seq {seq} committed as last-good")
            }
            AckOutcome::Stale(seq) => {
                log::warn!("hot-update: ack for seq {seq} no longer matches on-disk state; ignored")
            }
            AckOutcome::AlreadyCommitted(_) | AckOutcome::EmbeddedNoop => {}
        }
        Ok(outcome)
    }

    /// Những gì đang được phục vụ ngay lúc này.
    ///
    /// What is being served right now.
    pub fn current_bundle(&self) -> Result<CurrentBundle> {
        let activation = self.shared.activation()?;
        Ok(match activation.active {
            Active::Ota(seq) => CurrentBundle {
                source: BundleSource::Ota,
                seq: Some(seq),
                version: activation.active_version.clone(),
            },
            Active::Embedded => CurrentBundle {
                source: BundleSource::Embedded,
                seq: None,
                version: activation.embedded_version.clone(),
            },
        })
    }

    /// Lối thoát khẩn cấp để debug/hỗ trợ: xóa toàn bộ trạng thái OTA và các gói bundle, khởi động lại
    /// mốc watermark từ phiên bản nhúng. Thư mục gói hiện đang phục vụ
    /// (nếu có) được giữ nguyên vị trí cho đến khi GC ở lần khởi động sau dọn dẹp để webview
    /// đang chạy không bị rách giao diện; việc phục vụ sẽ quay về gói nhúng ở lần khởi chạy tiếp theo.
    ///
    /// ---
    /// Debug/support escape hatch: wipe all OTA state and bundles, restart
    /// the watermark from the embedded version. The currently served bundle
    /// dir (if any) is left in place until the next boot's GC so the running
    /// webview is not torn; serving reverts to embedded on the next launch.
    pub fn reset(&self) -> Result<()> {
        let activation = self.shared.activation()?;
        let effects = activation.store.update(|state| {
            machine::reset(state, &activation.embedded_version, activation.active)
        })?;
        activation.store.apply_effects(&effects);
        activation.store.sweep_foreign_entries();
        log::info!("hot-update: state reset; embedded serving resumes next launch");
        Ok(())
    }
}

#[cfg(test)]
mod tests;
