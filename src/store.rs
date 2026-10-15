//! Kho lưu trữ trên đĩa: lớp vỏ I/O mỏng bao quanh máy trạng thái thuần túy.
//!
//! Cấu trúc thư mục bên trong app data dir (tài liệu thiết kế, "On-disk layout"):
//!
//! ```text
//! hot-update/
//!   bundles/seq-<N>/    # mỗi thư mục chứa một gói đã giải nén
//!   state.json          # machine::State được lưu bền vững
//! ```
//!
//! Các quy tắc:
//! - `state.json` được ghi theo cơ chế nguyên tử (tệp tạm trong cùng thư mục + fsync +
//!   đổi tên), do đó sự cố crash giữa chừng khi ghi không bao giờ để lại một con trỏ ghi dở dang.
//! - Một tệp `state.json` bị thiếu, bị cắt cụt hoặc không phân tích cú pháp được sẽ được nạp như một
//!   [`State`] mới tinh — không bao giờ hoảng loạn/panic. Khi đó ứng dụng sẽ phục vụ gói nhúng
//!   (mức sàn an toàn dự phòng) và máy trạng thái sẽ xây dựng lại trạng thái từ đó.
//! - Mọi thứ bên dưới `bundles/` không phải là thư mục `seq-<N>` được tham chiếu đều là
//!   rác do plugin sở hữu (các mục mồ côi do ứng dụng bị tắt ngang giữa lúc giải nén và ghi trạng thái,
//!   thư mục tạm cũ) và sẽ được GC quét dọn khi khởi động.
//! - Các seq được cấp phát cao hơn mọi seq từng thấy trên đĩa *hoặc* trong trạng thái,
//!   vì vậy một thư mục mồ côi cũ không bao giờ trùng tên với một gói trong tương lai.
//!
//! ---
//! On-disk store: the thin I/O shell around the pure state machine.
//!
//! Layout inside the app data dir (design doc, "On-disk layout"):
//!
//! ```text
//! hot-update/
//!   bundles/seq-<N>/    # one dir per extracted bundle
//!   state.json          # the persisted machine::State
//! ```
//!
//! Rules:
//! - `state.json` is written atomically (temp file in the same dir + fsync +
//!   rename), so a crash mid-write can never leave a half-written pointer.
//! - A missing, truncated, or otherwise unparseable `state.json` loads as a
//!   fresh [`State`] — never a panic. The app then serves embedded (the
//!   fail-safe floor) and the machine rebuilds state from there.
//! - Everything under `bundles/` that is not a referenced `seq-<N>` dir is
//!   plugin-owned garbage (orphans from a kill between extraction and state
//!   write, stale temp dirs) and is swept by boot GC.
//! - Seqs are allocated above every seq ever seen on disk *or* in the state,
//!   so a stale orphan dir can never alias a future bundle.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::machine::{Effect, State};

const STATE_FILE: &str = "state.json";
const BUNDLES_DIR: &str = "bundles";
const SEQ_PREFIX: &str = "seq-";

/// Bộ xử lý trỏ tới thư mục `hot-update/`. Mọi biến đổi trạng thái đều đi qua
/// [`Store::update`], giúp tuần tự hóa các chu kỳ nạp→chuyển đổi→lưu sau
/// một mutex để lệnh xác nhận (ack) giữa phiên và một tiến trình tải xuống sắp xong không thể làm mất các bản cập nhật.
///
/// ---
/// Handle to the `hot-update/` directory. All state mutations go through
/// [`Store::update`], which serializes load→transition→persist cycles behind
/// a mutex so a mid-session ack and a finishing download cannot lose updates.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    write_lock: Mutex<()>,
}

impl Store {
    /// `root` là chính thư mục `hot-update/` (được tạo lười khi cần).
    ///
    /// `root` is the `hot-update/` directory itself (created lazily).
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            write_lock: Mutex::new(()),
        }
    }

    #[cfg(test)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(STATE_FILE)
    }

    /// Thư mục `bundles/`: nơi lưu trữ cuối cùng của `seq-<N>` và các tệp/thư mục
    /// tạm thời của quy trình cập nhật (bất kỳ mục nào không phải `seq-<N>` ở đây
    /// đều được GC quét sạch lúc khởi động, giúp các lần tải bị crash tự dọn dẹp).
    ///
    /// The `bundles/` directory: final `seq-<N>` homes and the update
    /// pipeline's temp files/dirs (anything non-`seq-<N>` here is swept by
    /// boot GC, which is what makes crashed downloads self-cleaning).
    pub(crate) fn bundles_dir(&self) -> PathBuf {
        self.root.join(BUNDLES_DIR)
    }

    /// Thư mục chứa một seq gói (`hot-update/bundles/seq-<N>`).
    ///
    /// Directory a bundle seq lives in (`hot-update/bundles/seq-<N>`).
    pub fn bundle_dir(&self, seq: u64) -> PathBuf {
        self.bundles_dir().join(format!("{SEQ_PREFIX}{seq}"))
    }

    /// Đọc nạp `state.json`. Các tệp bị thiếu hoặc bị hỏng sẽ trả về trạng thái mới tinh —
    /// việc phục hồi là "phục vụ gói nhúng và làm lại từ đầu", không bao giờ hoảng loạn/panic.
    ///
    /// Load `state.json`. Missing or corrupt files yield a fresh state —
    /// recovery is "serve embedded and start over", never a panic.
    pub fn load_state(&self) -> State {
        let path = self.state_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return State::default(),
            Err(e) => {
                log::warn!(
                    "hot-update: failed to read {}: {e}; starting fresh",
                    path.display()
                );
                return State::default();
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(state) => state,
            Err(e) => {
                log::warn!(
                    "hot-update: corrupt state.json ({e}); discarding it and starting fresh"
                );
                State::default()
            }
        }
    }

    /// Lưu trữ bền vững trạng thái theo cơ chế nguyên tử: ghi vào `state.json.tmp` trong cùng
    /// thư mục, fsync, sau đó đổi tên ghi đè lên `state.json`.
    ///
    /// Persist the state atomically: write `state.json.tmp` in the same
    /// directory, fsync, then rename over `state.json`.
    pub fn save_state(&self, state: &State) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let path = self.state_path();
        let tmp = self.root.join(format!("{STATE_FILE}.tmp"));
        let json = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
        {
            let mut file = fs::File::create(&tmp)?;
            io::Write::write_all(&mut file, &json)?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &path)
    }

    /// Tuần tự hóa quy trình nạp → chuyển đổi → lưu. Quá trình chuyển đổi chạy trên
    /// trạng thái mới nhất trên đĩa và kết quả của nó được lưu trước khi trả về.
    ///
    /// Serialized load → transition → persist. The transition runs on the
    /// freshest on-disk state and its result is persisted before returning.
    pub fn update<T>(&self, transition: impl FnOnce(State) -> (State, T)) -> io::Result<T> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let (state, out) = transition(self.load_state());
        self.save_state(&state)?;
        Ok(out)
    }

    /// Danh sách các seq của các thư mục `seq-<N>` đang tồn tại trong `bundles/`.
    ///
    /// Seqs of the `seq-<N>` dirs that exist under `bundles/`.
    pub fn present_seqs(&self) -> BTreeSet<u64> {
        let Ok(entries) = fs::read_dir(self.bundles_dir()) else {
            return BTreeSet::new();
        };
        entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| parse_seq(&entry.file_name().to_string_lossy()))
            .collect()
    }

    /// Các mục bên dưới `bundles/` không phải là thư mục `seq-<N>` — các thư mục
    /// tạm cũ và mảnh vụn rác còn sót lại từ các quá trình giải nén bị gián đoạn.
    ///
    /// Entries under `bundles/` that are not `seq-<N>` dirs — stale temp
    /// dirs and other debris from interrupted extractions.
    pub fn foreign_entries(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.bundles_dir()) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                parse_seq(&name).is_none() || !entry.path().is_dir()
            })
            .map(|entry| entry.path())
            .collect()
    }

    /// Seq tiếp theo: lớn hơn hẳn mọi thứ mà trạng thái tham chiếu và
    /// mọi thứ hiện có trên đĩa, nhờ vậy các thư mục mồ côi không bao giờ bị trùng tên.
    ///
    /// Next seq: strictly above everything the state references and
    /// everything present on disk, so orphan dirs are never aliased.
    pub fn allocate_seq(&self, state: &State) -> u64 {
        let disk_max = self.present_seqs().into_iter().max().unwrap_or(0);
        let state_max = [
            state.committed,
            state.last_good,
            state.staged,
            state.booting,
        ]
        .into_iter()
        .flatten()
        .chain(state.versions.keys().copied())
        .max()
        .unwrap_or(0);
        disk_max.max(state_max) + 1
    }

    /// Thực thi các hiệu ứng xóa, theo nỗ lực tối đa (best-effort): việc xóa thất bại sẽ được ghi log
    /// và để lại cho GC ở lần khởi động sau dọn dẹp.
    ///
    /// Execute deletion effects, best-effort: a failed delete is logged and
    /// left for the next boot's GC.
    pub fn apply_effects(&self, effects: &[Effect]) {
        for effect in effects {
            match effect {
                Effect::DeleteBundle(seq) => self.delete_path(&self.bundle_dir(*seq)),
            }
        }
    }

    /// Dọn sạch rác lạ (không phải `seq-<N>`) bên dưới `bundles/`.
    ///
    /// Remove foreign (non-`seq-<N>`) debris under `bundles/`.
    pub fn sweep_foreign_entries(&self) {
        for path in self.foreign_entries() {
            self.delete_path(&path);
        }
    }

    fn delete_path(&self, path: &Path) {
        let result = if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
        match result {
            Ok(()) => log::debug!("hot-update: removed {}", path.display()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => log::warn!(
                "hot-update: failed to remove {} ({e}); leaving it for next boot's GC",
                path.display()
            ),
        }
    }
}

fn parse_seq(name: &str) -> Option<u64> {
    name.strip_prefix(SEQ_PREFIX)?.parse().ok()
}

#[cfg(test)]
mod tests;
