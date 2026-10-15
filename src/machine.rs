//! Máy trạng thái khôi phục (rollback state machine), viết bằng Rust thuần túy.
//!
//! Mỗi bước chuyển đổi nhận [`State`] hiện tại cùng một sự kiện và trả về trạng
//! thái mới cùng với các [`Effect`] để vỏ I/O ([`crate::store`], [`crate::runtime`])
//! thực thi. **Không có I/O nào diễn ra ở đây** — điều này giúp mọi kịch bản
//! chống treo/hỏng (anti-brick) đều có thể kiểm thử đơn vị được.
//!
//! Con trỏ ba trạng thái (tài liệu thiết kế, "State machine"):
//!
//! ```text
//! download ──▶ staged ──(khởi động nguội tiếp theo, lưu TRƯỚC KHI phục vụ)──▶ booting
//!                            booting ──(xác nhận notifyAppReady)──────▶ committed
//!                            booting ──(lần khởi động 1, không xác nhận)──▶ booting (tái kích hoạt)
//!                            booting ──(lần khởi động 2, không xác nhận)──▶ failed (theo sha256 gói nén)
//! ```
//!
//! Các bất biến được thực thi tại đây:
//! - Việc thiếu xác nhận (ack) **chính là** tín hiệu thất bại (không dùng bộ đếm thời gian khởi động):
//!   con trỏ `booting` được tìm thấy lúc khởi động đồng nghĩa với việc lần khởi chạy trước không bao giờ gửi ack.
//!   Khởi chạy trên desktop thường ít và phiên dùng kéo dài, nên một lần bỏ lỡ duy nhất chỉ *tái kích hoạt*
//!   cùng gói đó cho một lần thử nữa; lần khởi chạy thứ hai liên tiếp không có ack sẽ đưa mã sha256 gói nén
//!   vào danh sách đen và việc phục vụ quay về committed/embedded. Một gói mà frontend nhúng mới hơn đã đuổi kịp
//!   sẽ bị loại bỏ ngay (không đưa vào danh sách đen) — frontend nhúng sẽ được ưu tiên.
//! - `max_version_seen` là một mốc chuẩn đơn điệu tăng (bảo vệ chống hạ cấp-phát lại và frontend nhúng mới hơn OTA).
//!   Nó chỉ có thể tăng lên.
//! - Các mã băm gói nén thất bại sẽ không bao giờ được chuyển vào staged hoặc kích hoạt lại.
//! - Lệnh ack sẽ commit seq mà tiến trình thực sự đã khởi động (được truyền vào bởi bên gọi từ bộ nhớ),
//!   không bao giờ đọc lại giá trị từ đĩa — một tiến trình tải xuống hoàn tất giữa phiên không được phép commit khi chưa thử nghiệm.
//!
//! ---
//! The rollback state machine, as pure Rust.
//!
//! Every transition takes the current [`State`] plus an event and returns the
//! new state together with [`Effect`]s for the I/O shell ([`crate::store`],
//! [`crate::runtime`]) to execute. **No I/O happens here** — that is what
//! makes every anti-brick scenario unit-testable.
//!
//! The three-state pointer (design doc, "State machine"):
//!
//! ```text
//! download ──▶ staged ──(next cold boot, persisted BEFORE serving)──▶ booting
//!                            booting ──(notifyAppReady ack)──────▶ committed
//!                            booting ──(1st boot, no ack)──▶ booting (re-armed)
//!                            booting ──(2nd boot, no ack)──▶ failed (by archive sha256)
//! ```
//!
//! Invariants enforced here:
//! - Absence of the ack **is** the failure signal (no boot timer): a `booting`
//!   pointer found at boot means the previous launch never acked. Desktop
//!   launches are rare and sessions long, so a single miss only *re-arms* the
//!   same bundle for one more trial; a *second* consecutive unacked launch
//!   blacklists its archive sha256 and serving falls back to
//!   committed/embedded. A bundle a newer embedded frontend has caught up to
//!   is discarded outright (not blacklisted) — the embedded frontend wins.
//! - `max_version_seen` is a monotonic watermark (downgrade-replay and
//!   embedded-newer-than-OTA protection). It only ever rises.
//! - Failed archive hashes are never staged or armed again.
//! - The ack commits the seq the process actually booted (passed in by the
//!   caller from memory), never a value re-read from disk — a download that
//!   finishes mid-session must not be committed untried.

use std::collections::{BTreeMap, BTreeSet};

use semver::Version;
use serde::{Deserialize, Serialize};

/// Siêu dữ liệu theo từng gói được ghi nhận tại thời điểm chuyển vào staged.
///
/// `archive_sha256` là định danh dùng cho danh sách đen thất bại: một bản
/// triển khai sửa lỗi sẽ mang mã băm mới, vì vậy việc đưa vào danh sách đen
/// theo mã băm không bao giờ chặn bản vá.
///
/// ---
/// Per-bundle metadata recorded at staging time.
///
/// `archive_sha256` is the identity used for the failure blacklist: a fixed
/// deploy ships under a new hash, so blacklisting by hash never blocks a fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleMeta {
    pub version: Version,
    pub archive_sha256: String,
}

/// Nội dung được lưu trữ bền vững của `state.json`.
///
/// Được tuần tự hóa dưới dạng camelCase theo tài liệu thiết kế. Các trường không xác định
/// từ các phiên bản plugin mới hơn sẽ bị bỏ qua khi đọc (tương thích tiến); một tệp
/// hoàn toàn không thể phân tích cú pháp sẽ được store xem như mới tinh (không bao giờ hoảng loạn/panic).
///
/// ---
/// The persisted contents of `state.json`.
///
/// Serialized as camelCase per the design doc. Unknown fields from newer
/// plugin versions are ignored on read (forward compatibility); a file that
/// fails to parse at all is treated as fresh by the store (never a panic).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    /// Gói cuối cùng đã hoàn tất khởi động thử nghiệm và được xác nhận (acked).
    ///
    /// The last bundle that completed a trial boot and was acked.
    pub committed: Option<u64>,
    /// Gói tốt gần nhất đã biết. Di chuyển cùng với `committed` trong v1; được giữ
    /// lại dưới dạng con trỏ riêng biệt theo thiết kế cho các luồng phục hồi trong tương lai.
    ///
    /// Last-known-good bundle. Moves together with `committed` in v1; kept
    /// as a separate pointer per the design for future recovery flows.
    pub last_good: Option<u64>,
    /// Đã tải xuống và giải nén, đang chờ lần khởi động nguội tiếp theo.
    ///
    /// Downloaded and extracted, waiting for the next cold boot.
    pub staged: Option<u64>,
    /// Được trang bị cho lần khởi động thử nghiệm. Xuất hiện lúc khởi động ⇒ lần thử trước
    /// chưa từng được ack.
    ///
    /// Armed for a trial boot. Present at boot time ⇒ the previous trial
    /// never acked.
    pub booting: Option<u64>,
    /// Số lần khởi chạy không có ack liên tiếp của gói `booting` hiện tại. Khởi chạy trên
    /// desktop thường ít và phiên kéo dài, do đó một gói *tốt* mà tiến trình bị thoát trước
    /// khi gọi `notifyAppReady()` không được phép bị đưa vào danh sách đen chỉ sau một lần
    /// lỡ: gói được tái trang bị một lần (strike 1) và chỉ bị đưa vào danh sách đen sau
    /// lần khởi chạy thứ hai liên tiếp không có ack (strike 2). Được đặt lại về 0 khi commit
    /// và bất cứ khi nào một gói mới được thăng cấp từ `staged`. `#[serde(default)]` để tệp
    /// `state.json` cũ (thiếu trường này) tải lên với giá trị 0.
    ///
    /// Consecutive unacked launches of the current `booting` bundle. Desktop
    /// launches are rare and sessions long, so a *good* bundle whose process
    /// quit before `notifyAppReady()` must not be blacklisted on a single
    /// miss: the bundle is re-armed once (strike 1) and only blacklisted after
    /// a *second* consecutive unacked launch (strike 2). Reset to 0 on commit
    /// and whenever a fresh bundle is promoted from `staged`. `#[serde(default)]`
    /// so a pre-2-strike `state.json` (which lacks the field) loads as 0.
    #[serde(default)]
    pub booting_strikes: u32,
    /// Danh sách đen các mã băm sha256 của gói nén đã thất bại trong lần khởi động thử.
    ///
    /// Blacklist of archive sha256 hashes that failed a trial boot.
    pub failed: BTreeSet<String>,
    /// Mốc chuẩn đơn điệu: max(phiên bản nhúng, phiên bản OTA cao nhất đã commit).
    /// Các manifest/gói bằng hoặc thấp hơn mốc này sẽ bị từ chối.
    ///
    /// Monotonic watermark: max(embedded version, highest committed OTA
    /// version). Manifests/bundles at or below it are refused.
    pub max_version_seen: Option<Version>,
    /// seq → siêu dữ liệu cho mọi gói mà trạng thái vẫn tham chiếu đến.
    ///
    /// seq → metadata for every bundle the state still references.
    pub versions: BTreeMap<u64, BundleMeta>,
}

impl State {
    /// Các seq mà trạng thái vẫn tham chiếu sau quá trình chuyển đổi — mọi thứ khác
    /// trên đĩa đều là rác.
    ///
    /// Seqs the state still references after a transition — everything else
    /// on disk is garbage.
    fn referenced(&self) -> BTreeSet<u64> {
        [self.committed, self.last_good, self.staged, self.booting]
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Nguồn mà provider phục vụ trong suốt vòng đời tiến trình này.
///
/// Which source the provider serves this process lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Active {
    Embedded,
    Ota(u64),
}

/// Các hiệu ứng phụ (side effects) để shell thực thi sau khi lưu trạng thái mới.
/// Việc xóa file theo nguyên tắc nỗ lực tối đa (best-effort): nếu lỗi sẽ để lại
/// thư mục mồ côi mà bộ thu gom rác (GC) ở lần khởi động tiếp theo sẽ dọn dẹp.
///
/// ---
/// Side effects for the shell to execute after persisting the new state.
/// Deletions are best-effort: a failure leaves an orphan dir that the next
/// boot's GC removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    DeleteBundle(u64),
}

/// Kết quả phân giải tại thời điểm khởi động.
///
/// Result of boot-time resolution.
#[derive(Debug)]
pub struct BootOutcome {
    pub state: State,
    /// Dữ liệu mà tiến trình này phải phục vụ. Shell phải lưu `state` TRƯỚC KHI
    /// phục vụ dù chỉ một byte khi đây là một gói mới được trang bị
    /// (`state.booting`), nếu không một vụ crash sẽ lọt qua khỏi cơ chế phát hiện rollback.
    ///
    /// What this process must serve. The shell must persist `state` BEFORE
    /// serving a single byte when this is a newly armed bundle
    /// (`state.booting`), otherwise a crash would evade rollback detection.
    pub active: Active,
    /// Seq bị quy trách nhiệm cho lần khởi động thử nghiệm không có ack trước đó, nếu có.
    ///
    /// Seq blamed for the previous unacked trial boot, if any.
    pub rolled_back: Option<u64>,
    pub effects: Vec<Effect>,
}

/// Phân giải lúc khởi động: phát hiện rollback, tích hợp mốc chuẩn watermark,
/// loại bỏ gói khi frontend nhúng mới hơn OTA, thăng cấp staged→booting, thu gom rác (GC).
///
/// `present` là tập hợp các thư mục gói thực sự tồn tại trên đĩa; các con trỏ
/// trỏ tới thư mục vắng mặt sẽ bị hủy bỏ thay vì được trang bị/phục vụ.
///
/// ---
/// Boot-time resolution: rollback detection, watermark fold-in,
/// embedded-newer-than-OTA discard, staged→booting promotion, GC.
///
/// `present` is the set of bundle dirs that actually exist on disk; pointers
/// to absent dirs are dropped rather than armed/served.
pub fn resolve_boot(
    mut state: State,
    embedded_version: &Version,
    present: &BTreeSet<u64>,
) -> BootOutcome {
    // 1. Unacked trial boot from a previous launch. The bundle pointed at by
    //    `booting` was armed last launch and never acked. Two-strike softening
    //    (design §4, "desktop-hardened rollback"): desktop launches are rare
    //    and a good bundle is often quit before `notifyAppReady()`, so a
    //    single miss must not condemn it.
    let mut rolled_back = None;
    if let Some(seq) = state.booting.take() {
        let meta = state.versions.get(&seq).cloned();
        let superseded = meta
            .as_ref()
            .is_some_and(|m| m.version <= *embedded_version);

        if superseded {
            // A newer embedded frontend (e.g. a Sparkle/native update that
            // landed while this trial was mid-flight) has caught up to the
            // trial bundle. It is stale, not broken: discard it (GC sweeps the
            // dir) and let embedded win — no blacklist, not a failure rollback.
            state.booting_strikes = 0;
        } else if state.booting_strikes == 0 && present.contains(&seq) && meta.is_some() {
            // Strike 1: still applicable and on disk. Give it exactly one more
            // launch to ack before condemning it. `booting` stays occupied, so
            // step 4 will not promote a waiting staged bundle over it.
            state.booting = Some(seq);
            state.booting_strikes = 1;
        } else {
            // Strike 2 (a genuine second unacked launch), a bundle whose dir
            // vanished, or a corrupt pointer with no metadata: this bundle can
            // never be trusted again. Blacklist its archive sha256 when known
            // (a fixed redeploy ships under a new hash and is unaffected) and
            // roll back to committed/embedded.
            if let Some(m) = &meta {
                state.failed.insert(m.archive_sha256.clone());
            }
            state.booting_strikes = 0;
            rolled_back = Some(seq);
        }
    }

    // 2. Fold the embedded version into the monotonic watermark.
    state.max_version_seen = Some(match state.max_version_seen.take() {
        Some(seen) => seen.max(embedded_version.clone()),
        None => embedded_version.clone(),
    });

    // 3. Embedded-newer-than-OTA discard: a store update whose embedded
    //    frontend is at least as new as the committed OTA bundle wins, and a
    //    committed pointer whose dir vanished cannot be served.
    let ota_beats_embedded = |seq: u64, state: &State| -> bool {
        present.contains(&seq)
            && state
                .versions
                .get(&seq)
                .is_some_and(|meta| meta.version > *embedded_version)
    };
    if let Some(seq) = state.committed {
        if !ota_beats_embedded(seq, &state) {
            state.committed = None;
        }
    }
    if let Some(seq) = state.last_good {
        if !ota_beats_embedded(seq, &state) {
            state.last_good = None;
        }
    }

    // 4. Staged promotion, re-gated against the *updated* watermark and the
    //    blacklist. The download gates already checked these, but arming is
    //    the last line of defense and the situation can legitimately change
    //    between staging and boot (e.g. a store update raised the watermark).
    //    Skipped while `booting` is still occupied by a strike-1 re-arm: the
    //    queued bundle stays staged and waits for the next boot rather than
    //    jumping ahead of the bundle currently on its second trial.
    let promote = if state.booting.is_some() {
        None
    } else {
        state.staged.take()
    };
    if let Some(seq) = promote {
        let arm = present.contains(&seq)
            && state.versions.get(&seq).is_some_and(|meta| {
                !state.failed.contains(&meta.archive_sha256)
                    && state
                        .max_version_seen
                        .as_ref()
                        .map_or(true, |seen| meta.version > *seen)
            });
        if arm {
            state.booting = Some(seq);
            // A freshly promoted bundle starts its trial with a clean strike
            // count, even if a leftover count somehow survived a prior commit.
            state.booting_strikes = 0;
        }
        // Not armable ⇒ silently discarded; GC removes the dir.
    }

    // 5. Active source for this process lifetime.
    let active = match state.booting.or(state.committed) {
        Some(seq) => Active::Ota(seq),
        None => Active::Embedded,
    };

    // 6. GC: every seq on disk or in the versions map that the final state
    //    no longer references is garbage (orphans from a kill between
    //    extraction and state write, rolled-back bundles, discarded staged).
    let referenced = state.referenced();
    let version_seqs: BTreeSet<u64> = state.versions.keys().copied().collect();
    let mut effects = Vec::new();
    for seq in present.union(&version_seqs).copied() {
        if !referenced.contains(&seq) {
            if present.contains(&seq) {
                effects.push(Effect::DeleteBundle(seq));
            }
            state.versions.remove(&seq);
        }
    }

    BootOutcome {
        state,
        active,
        rolled_back,
        effects,
    }
}

/// Kết quả của một lệnh xác nhận ([`ack`]).
///
/// Result of an ack ([`ack`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckOutcome {
    /// Gói thử nghiệm hiện đã được commit và là gói tốt gần nhất.
    ///
    /// The trial bundle is now committed and last-good.
    Committed(u64),
    /// Trạng thái ổn định: gói đã khởi động vốn đã được commit (mọi lần khởi chạy
    /// đều gọi `notifyAppReady`, và lệnh gọi này có tính lũy đẳng).
    ///
    /// Steady state: the booted bundle was already committed (every launch
    /// calls `notifyAppReady`, and the call is idempotent).
    AlreadyCommitted(u64),
    /// Đang phục vụ gói nhúng — không có gì để commit.
    ///
    /// Serving embedded — nothing to commit.
    EmbeddedNoop,
    /// Seq đã khởi động không còn khớp với con trỏ `booting` trên đĩa (ví dụ:
    /// một lệnh `reset()` đã chạy giữa phiên). Từ chối commit là giải pháp an toàn.
    ///
    /// The booted seq no longer matches the on-disk `booting` pointer (e.g.
    /// a `reset()` ran mid-session). Refusing to commit is the safe answer.
    Stale(u64),
}

/// `notifyAppReady`: commit gói mà tiến trình này đã khởi động.
///
/// `booted` BẮT BUỘC phải là giá trị trong bộ nhớ được ghi lại khi phân giải khởi động — không bao giờ
/// đọc lại từ đĩa, vì việc tải xuống hoàn tất giữa phiên có thể đã
/// ghi đè lại `staged` (và `staged` phải được giữ nguyên vẹn cho lần khởi động tiếp theo).
///
/// ---
/// `notifyAppReady`: commit the bundle this process booted.
///
/// `booted` MUST be the in-memory value captured at boot resolution — never
/// re-read from disk, because a download finishing mid-session may have
/// re-written `staged` (and `staged` must stay untouched for the next boot).
pub fn ack(mut state: State, booted: Active) -> (State, AckOutcome, Vec<Effect>) {
    let seq = match booted {
        Active::Embedded => return (state, AckOutcome::EmbeddedNoop, Vec::new()),
        Active::Ota(seq) => seq,
    };
    if state.booting != Some(seq) {
        let outcome = if state.committed == Some(seq) {
            AckOutcome::AlreadyCommitted(seq)
        } else {
            AckOutcome::Stale(seq)
        };
        return (state, outcome, Vec::new());
    }

    state.booting = None;
    // A bundle that acks once is good: clear any strike accumulated during a
    // re-armed trial so it can never contaminate the next bundle's trial.
    state.booting_strikes = 0;
    let previous = state.committed.replace(seq);
    state.last_good = Some(seq);
    if let Some(meta) = state.versions.get(&seq) {
        state.max_version_seen = Some(match state.max_version_seen.take() {
            Some(seen) => seen.max(meta.version.clone()),
            None => meta.version.clone(),
        });
    }

    // The bundle this one replaces is no longer referenced (unless a pointer
    // still names it, e.g. it is also the staged seq in a corrupt file).
    let mut effects = Vec::new();
    if let Some(prev) = previous {
        if !state.referenced().contains(&prev) {
            state.versions.remove(&prev);
            effects.push(Effect::DeleteBundle(prev));
        }
    }

    (state, AckOutcome::Committed(seq), effects)
}

/// Lý do tại sao yêu cầu đưa vào staged bị từ chối.
///
/// Why a staging request was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StageError {
    #[error("bundle version {version} is not newer than the watermark {watermark}")]
    VersionNotNewer {
        version: Version,
        watermark: Version,
    },
    #[error("archive sha256 {0} previously failed a trial boot and is blacklisted")]
    HashBlacklisted(String),
    #[error("seq {0} is already referenced by the state")]
    SeqInUse(u64),
}

/// Đưa gói vừa được giải nén vào staged (bước cuối cùng của bộ tải xuống WP3).
///
/// Điều kiện kiểm soát: phiên bản phải cao hơn hẳn so với mốc watermark (bảo vệ
/// chống hạ cấp-phát lại) và mã băm gói nén không nằm trong danh sách đen. Việc thay thế
/// một gói đã staged trước đó nhưng chưa từng được trang bị sẽ biến nó thành mồ côi (hiệu ứng xóa).
///
/// ---
/// Stage a freshly extracted bundle (the WP3 downloader's final step).
///
/// Gates: the version must be strictly above the watermark (downgrade-replay
/// protection) and the archive hash must not be blacklisted. Replacing an
/// earlier staged-but-never-armed bundle orphans it (delete effect).
pub fn stage(
    mut state: State,
    seq: u64,
    meta: BundleMeta,
) -> (State, Result<Vec<Effect>, StageError>) {
    if let Some(watermark) = &state.max_version_seen {
        if meta.version <= *watermark {
            let err = StageError::VersionNotNewer {
                version: meta.version,
                watermark: watermark.clone(),
            };
            return (state, Err(err));
        }
    }
    if state.failed.contains(&meta.archive_sha256) {
        let err = StageError::HashBlacklisted(meta.archive_sha256);
        return (state, Err(err));
    }
    if state.referenced().contains(&seq) || state.versions.contains_key(&seq) {
        return (state, Err(StageError::SeqInUse(seq)));
    }

    let mut effects = Vec::new();
    if let Some(prev) = state.staged.replace(seq) {
        state.versions.remove(&prev);
        effects.push(Effect::DeleteBundle(prev));
    }
    state.versions.insert(seq, meta);
    (state, Ok(effects))
}

/// `reset()`: lối thoát khẩn cấp để debug/hỗ trợ — trạng thái xuất xưởng, phục vụ
/// gói nhúng, mốc watermark được khởi động lại từ phiên bản nhúng.
///
/// Gói mà tiến trình *hiện đang phục vụ* (nếu có) được chủ động giữ lại không
/// xóa — việc giật mất tệp ngay dưới webview đang chạy có thể làm hỏng giao diện
/// người dùng. Nó không còn được trạng thái mới tham chiếu, nên GC của lần khởi động tới sẽ dọn dẹp nó.
///
/// ---
/// `reset()`: the debug/support escape hatch — factory state, embedded
/// serving, watermark restarted from the embedded version.
///
/// The bundle the process is *currently serving* (if any) is deliberately not
/// deleted — yanking files out from under the running webview could tear the
/// UI. It is unreferenced by the fresh state, so the next boot's GC sweeps it.
pub fn reset(state: State, embedded_version: &Version, active: Active) -> (State, Vec<Effect>) {
    let keep = match active {
        Active::Embedded => None,
        Active::Ota(seq) => Some(seq),
    };
    let effects = state
        .versions
        .keys()
        .copied()
        .filter(|seq| Some(*seq) != keep)
        .map(Effect::DeleteBundle)
        .collect();
    let fresh = State {
        max_version_seen: Some(embedded_version.clone()),
        ..State::default()
    };
    (fresh, effects)
}

#[cfg(test)]
mod tests;
