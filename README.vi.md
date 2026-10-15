# tauri-plugin-hot-update

[English](README.md) | [Tiếng Việt](README.vi.md)

[![crates.io](https://img.shields.io/crates/v/tauri-plugin-hot-update.svg)](https://crates.io/crates/tauri-plugin-hot-update)
[![npm](https://img.shields.io/npm/v/tauri-plugin-hot-update-api.svg)](https://www.npmjs.com/package/tauri-plugin-hot-update-api)
[![docs.rs](https://img.shields.io/docsrs/tauri-plugin-hot-update)](https://docs.rs/tauri-plugin-hot-update)
[![license](https://img.shields.io/crates/l/tauri-plugin-hot-update.svg)](#license)

**Cập nhật nóng (Hot update) / Cập nhật trực tiếp qua mạng OTA (Over-The-Air) cho ứng dụng di động và máy tính để bàn chạy Tauri v2 — tự lưu trữ (self-hosted), theo phong cách CodePush, tích hợp cơ chế tự động hoàn tác chống brick (chống hỏng ứng dụng).**

`tauri-plugin-hot-update` mang lại khả năng **cập nhật nóng (hot update)** — **cập nhật qua mạng OTA**, **cập nhật trực tiếp (live update)** cho giao diện web frontend của bạn — áp dụng cho các ứng dụng **Tauri v2** trên **iOS, Android, macOS, Windows và Linux**. Đây là một kênh cập nhật **tự lưu trữ, theo mô hình CodePush / Expo-Updates** dành riêng cho Tauri: cho phép bạn đẩy các bản sửa lỗi JS/CSS/HTML, các bản **vá nóng (hotfix)** và cập nhật nội dung đến những thiết bị đã cài đặt ứng dụng chỉ trong vài giây mà không cần chờ App Store hoặc Google Play Store xét duyệt. Nếu bạn đang tìm kiếm một **giải pháp thay thế CodePush cho Tauri**, **Expo Updates cho Tauri**, **Tauri OTA update**, **Tauri live update**, **Tauri hotfix**, hay thuật ngữ tiếng Trung **Tauri 热更新** — thì đây chính là giải pháp đó, được phân phối trực tiếp từ máy chủ hoặc CDN của chính bạn mà không bị phụ thuộc vào bất kỳ dịch vụ SaaS bên thứ ba nào.

> **Trạng thái:** Đầy đủ tính năng và đã được kiểm thử đơn vị (unit test) toàn diện; phiên bản tiền 1.0 (pre-1.0). Xem mục [Hỗ trợ nền tảng](#hỗ-trợ-nền-tảng) để biết chi tiết các bài kiểm thử thực tế trên thiết bị.

## Mục lục

- [Tại sao cần plugin này?](#tại-sao-cần-plugin-này)
- [Tính năng & Mô hình an toàn](#tính-năng--mô-hình-an-toàn)
- [Cơ chế hoạt động](#cơ-chế-hoạt-động)
- [Cài đặt](#cài-đặt)
- [Hướng dẫn tích hợp nhanh](#hướng-dẫn-tích-hợp-nhanh)
- [Phát hành một gói bundle](#phát-hành-một-gói-bundle)
- [Hỗ trợ nền tảng](#hỗ-trợ-nền-tảng)
- [Tuân thủ chính sách kho ứng dụng (OTA có được phép trên iOS & Android không?)](#tuân-thủ-chính-sách-kho-ứng-dụng-ota-có-được-phép-trên-ios--android-không)
- [Bảo mật](#bảo-mật)
- [So sánh với các giải pháp khác](#so-sánh-với-các-giải-pháp-khác)
- [Giấy phép bản quyền](#giấy-phép-bản-quyền)

## Tại sao cần plugin này?

Thông thường, mọi thay đổi frontend trong một ứng dụng di động Tauri — kể cả chỉ là một dòng sửa CSS — đều đòi hỏi phải biên dịch lại toàn bộ mã nhị phân native, tải lên kho ứng dụng và trải qua chu kỳ xét duyệt trước khi người dùng có thể nhận được. Plugin chính thức [`tauri-plugin-updater`](https://v2.tauri.app/plugin/updater/) thay thế **toàn bộ** tệp nhị phân trên desktop và [không hỗ trợ nền tảng di động](https://github.com/orgs/tauri-apps/discussions/8467). Hệ sinh thái di động native đã giải quyết bài toán này từ nhiều năm trước với **CodePush** (React Native / Cordova) và **Expo Updates** (React Native) — phân phối ngầm tầng mã thông dịch (JS) qua mạng OTA. Plugin này mang đúng quy trình làm việc đó lên Tauri v2, hoạt động theo phương thức tự lưu trữ (self-hosted), đi kèm một mô hình hoàn tác (rollback) được thiết kế để một gói cập nhật lỗi không bao giờ có thể làm hỏng (brick) ứng dụng của người dùng.

## Tính năng & Mô hình an toàn

- **Phân phối các gói frontend qua OTA.** Thư mục `dist/` sau khi build (JS/CSS/HTML/tài nguyên) được đóng gói dạng tar, ký số và phân phối từ bất kỳ dịch vụ lưu trữ tĩnh nào (Cloudflare R2, AWS S3, CDN hoặc máy chủ riêng của bạn). Các ứng dụng đã cài đặt sẽ tải gói này ngầm dưới nền và áp dụng vào **lần khởi động nguội (cold launch) tiếp theo** — thời gian khởi chạy ứng dụng luôn tức thì và hoạt động hoàn hảo khi ngoại tuyến (offline), vì các tài nguyên luôn được phục vụ trực tiếp từ ổ đĩa cục bộ.
- **Gói cập nhật lỗi không bao giờ làm hỏng (brick) ứng dụng.** Cơ chế hoàn tác được quản lý bởi con trỏ ba trạng thái — **`staged → booting → committed`** — **hoàn toàn không dùng bộ đếm thời gian (timer) hay các phỏng đoán thời gian render**:
  1. Quá trình **Tải xuống (Download)** xác minh tính toàn vẹn và đưa gói vào trạng thái chuẩn bị một cách nguyên tử (`staged = N`).
  2. Ở lần khởi động nguội tiếp theo, trước khi webview được tạo, plugin sẽ nâng trạng thái `staged → booting` và **lưu bền vững trạng thái này xuống ổ đĩa trước khi phục vụ dù chỉ một byte** của gói mới.
  3. Frontend của bạn sẽ gọi hàm **`notifyAppReady()`** ngay sau khi giao diện khung ứng dụng (app shell) đã mount xong — thao tác này sẽ xác nhận (commit) gói đang chạy là bản hoạt động tốt gần nhất (last-known-good).
  4. Nếu trong lần khởi chạy, plugin phát hiện gói ở trạng thái `booting` chưa từng được xác nhận (do ứng dụng bị crash, màn hình trắng hoặc treo), mã băm (archive hash) của gói đó sẽ bị **đưa vào danh sách đen (blacklist)**, và hệ thống sẽ tự động hoàn tác về gói hoạt động tốt trước đó hoặc gói tài nguyên tĩnh nhúng sẵn ban đầu. Các gói có mã băm lỗi sẽ không bao giờ bị thử lại; bản sửa lỗi tiếp theo sẽ được phát hành dưới một mã băm mới.
- **Gói tài nguyên nhúng là chốt chặn an toàn vĩnh viễn.** Các tài nguyên tĩnh được biên dịch kèm vào tệp nhị phân nộp lên kho ứng dụng luôn là phương án dự phòng cuối cùng. Việc thiếu tín hiệu xác nhận sẵn sàng (ready-ack) *chính là* tín hiệu báo lỗi — được thiết kế độc lập hoàn toàn với kết nối mạng hay xác thực người dùng, giúp sự cố máy chủ backend không bao giờ dẫn đến việc đánh dấu hỏng hàng loạt gói bundle tốt trên toàn bộ thiết bị.
- **Tệp kê khai (manifest) có chữ ký số + Cơ chế chống tấn công hạ cấp/phát lại (downgrade-replay).** Mỗi tệp manifest đều được ký bằng [minisign](https://jedisct1.github.io/minisign/); ứng dụng tin tưởng một **danh sách** các khóa công khai (giúp dễ dàng luân chuyển khóa). Một con dấu phiên bản đơn điệu (monotonic version watermark) sẽ từ chối bất kỳ manifest cũ nào (dù chữ ký vẫn hợp lệ) bị phát lại bởi bộ đệm lưu trữ cũ hoặc kẻ tấn công đứng giữa (MITM).

## Cơ chế hoạt động

Tauri phục vụ frontend của bạn thông qua một điểm mở rộng duy nhất: `tauri::Context::assets`, một đối tượng công khai `Box<dyn Assets<R>>` có phương thức `get()` là nơi duy nhất xử lý lược đồ `tauri://` trên **mọi** nền tảng. Plugin này **thay thế nhà cung cấp tài nguyên đó** bằng một cơ chế mới: phân giải từng tài nguyên từ thư mục gói OTA đang hoạt động trên đĩa cục bộ, và tự động dự phòng về tài nguyên nhúng sẵn được biên dịch từ `frontendDist`.

Bằng cách tái sử dụng **lược đồ `tauri://` hiện có** thay vì đăng ký một lược đồ mới, nguồn gốc webview (origin) không bao giờ bị thay đổi — `tauri://localhost` trên iOS/macOS, `http://tauri.localhost` trên Android. Do đó, `localStorage`, `IndexedDB`, cookie, và trạng thái xác thực/phiên đăng nhập được **bảo toàn nguyên vẹn theo thiết kế**: bản cập nhật OTA sẽ không làm người dùng bị đăng xuất. (Việc đăng ký một lược đồ thứ hai *sẽ* làm thay đổi origin và âm thầm xóa sạch toàn bộ dữ liệu web storage — plugin này loại bỏ hoàn toàn rủi ro đó.)

Nhà cung cấp tài nguyên được kích hoạt bên trong hook thiết lập (setup hook) của plugin, nơi Tauri chạy **trước khi bất kỳ cửa sổ hay webview nào được khởi tạo** — do đó việc chuyển đổi trạng thái `staged → booting` được lưu chắc chắn xuống đĩa trước khi gói mới phục vụ bất kỳ nội dung nào, trên mọi nền tảng bao gồm cả Android (nơi thư mục dữ liệu ứng dụng thậm chí không thể xác định cho đến khi ứng dụng tồn tại).

## Cài đặt

### Rust

```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-hot-update = "0.1"
```

Hoặc theo dõi trực tiếp từ kho git:

```toml
tauri-plugin-hot-update = { git = "https://github.com/yanqianglu/tauri-plugin-hot-update" }
```

Backend TLS có thể tùy chọn, tương tự như `tauri-plugin-updater`. Cấu hình mặc định `rustls-tls` hỗ trợ biên dịch chéo (cross-compile) mượt mà cho `aarch64-apple-ios` và `aarch64-linux-android`; hãy chuyển sang `native-tls` nếu ứng dụng của bạn chuẩn hóa theo TLS của hệ điều hành:

```toml
tauri-plugin-hot-update = { version = "0.1", default-features = false, features = ["native-tls"] }
```

### JavaScript / TypeScript

```bash
npm install tauri-plugin-hot-update-api
# hoặc: pnpm add / bun add / yarn add
```

Yêu cầu `@tauri-apps/api` >= 2.0.0 dưới dạng peer dependency.

### Công cụ dòng lệnh ký phát hành (Release-signing CLI)

Tệp nhị phân `hot-update-sign` nằm dưới feature `cli` để toàn bộ mã minisign (xử lý khóa bí mật) không bao giờ bị đưa vào bản build ứng dụng của bạn:

```bash
cargo install tauri-plugin-hot-update --features cli
```

## Hướng dẫn tích hợp nhanh

### 1. Đổi nhà cung cấp tài nguyên + Đăng ký plugin (`src-tauri/src/main.rs`)

Việc tích hợp gồm **hai bước chia sẻ cùng một handle**. Việc hoán đổi tài nguyên bắt buộc phải diễn ra trên `Context` trước khi context bị tiêu thụ; việc phân giải đường dẫn chỉ hoạt động khi ứng dụng đang được dựng:

```rust
fn main() {
    let mut context = tauri::generate_context!();

    // Bước 1 — hoán đổi tài nguyên nhúng sang nhà cung cấp hot-update.
    let hot_update = tauri_plugin_hot_update::install(&mut context);

    tauri::Builder::default()
        // Bước 2 — đăng ký plugin ĐẦU TIÊN, để không thành phần nào truy cập tài nguyên
        // trước khi setup hook kích hoạt/hoàn tác kho lưu trữ bundle.
        .plugin(tauri_plugin_hot_update::init(hot_update))
        // ... các plugin khác của bạn
        .run(context)
        .expect("xảy ra lỗi khi chạy ứng dụng tauri");
}
```

### 2. Cấu hình nguồn cập nhật (`tauri.conf.json`)

```json
{
  "plugins": {
    "hot-update": {
      "manifestUrl": "https://updates.example.com/manifest.json",
      "pubkeys": ["RWT...khóa-công-khai-minisign-của-bạn..."],
      "enabled": true
    }
  }
}
```

- `manifestUrl` — đường dẫn URL trỏ đến tệp tĩnh (không kèm chuỗi truy vấn query string); chữ ký tách rời sẽ được tải từ `<manifestUrl>.minisig`.
- `pubkeys` — một hoặc nhiều khóa công khai minisign đáng tin cậy (chuỗi base64 `RW…` hoặc toàn bộ nội dung tệp `minisign.pub`). Khai báo cả khóa cũ **và** khóa mới trong quá trình luân chuyển khóa.
- `enabled` — đặt `false` nếu muốn phát hành thử nghiệm ẩn (dark-ship): plugin vẫn đăng ký nhưng không hoạt động, ứng dụng chạy hoàn toàn trên tài nguyên nhúng sẵn.

Cấu hình này là nguồn cập nhật **duy nhất** — mã JavaScript không thể truyền URL manifest hoặc khóa tùy ý, nhờ đó nội dung webview bị xâm phạm cũng không thể chuyển hướng nguồn cập nhật. Cấu hình bị sai định dạng sẽ dừng quá trình khởi động ngay trên máy của lập trình viên, tuyệt đối không bị lỗi âm thầm khi ra môi trường thực tế.

### 3. Cấp quyền thực thi các lệnh (Tệp capability)

```json
{ "permissions": ["hot-update:default"] }
```

Cấu hình này cấp quyền cho cả 5 lệnh: `check`, `download`, `notify_app_ready`, `current_bundle`, `reset`.

### 4. Điều khiển từ giao diện Frontend

```ts
import {
  check,
  download,
  notifyAppReady,
  currentBundle,
  reset,
  onDownloadProgress,
} from "tauri-plugin-hot-update-api";

// Trong mỗi lần khởi chạy, ngay sau khi khung ứng dụng đã mount và render xong:
// commit gói đang chạy là bản hoạt động tốt gần nhất. An toàn khi gọi
// vô điều kiện — không làm gì nếu đang dùng tài nguyên nhúng hoặc khi plugin bị tắt.
await notifyAppReady();

// Bất cứ khi nào ứng dụng quyết định kiểm tra bản cập nhật (lúc mở app/resume —
// plugin không bao giờ tự động thăm dò; bạn toàn quyền kiểm soát thời điểm):
const unlisten = await onDownloadProgress(({ downloaded, total }) => {
  console.log(`Tiến độ OTA: ${Math.round((downloaded / total) * 100)}%`);
});
const outcome = await download();
unlisten();

if (outcome.status === "staged") {
  // Bản cập nhật sẽ được áp dụng trong lần khởi động nguội tiếp theo.
  console.log(`Phiên bản v${outcome.version} đã sẵn sàng — hãy khởi động lại ứng dụng để áp dụng`);
}

// Kiểm tra thông tin & phương án khôi phục khẩn cấp:
await currentBundle(); // { source: "ota" | "embedded", seq, version }
await reset();         // xóa sạch trạng thái OTA, quay về tài nguyên nhúng ở lần khởi động kế
```

### Bề mặt API TypeScript

| Hàm | Kiểu trả về | Mục đích |
|---|---|---|
| `check()` | `CheckOutcome` | Tải về & xác thực manifest; kiểm tra xem có bản cập nhật nào áp dụng được không. Tuyệt đối không tự tải gói bundle. |
| `download()` | `DownloadOutcome` | Toàn bộ quy trình: kiểm tra → tải về → xác thực → giải nén → lưu vào hàng chờ cho lần khởi động kế tiếp. |
| `notifyAppReady()` | `AckOutcome` | Xác nhận (commit) gói đang khởi động là bản hoạt động tốt gần nhất. Gọi một lần mỗi khi ứng dụng khởi chạy. |
| `currentBundle()` | `CurrentBundle` | Lấy thông tin gói đang được phục vụ tại thời điểm hiện tại (`ota` hay `embedded`, seq, version). |
| `reset()` | `void` | Xóa toàn bộ trạng thái OTA; lần khởi động tiếp theo sẽ quay về tài nguyên nhúng. Dành cho việc gỡ lỗi / hỗ trợ kỹ thuật. |
| `onDownloadProgress(fn)` | `UnlistenFn` | Lắng nghe sự kiện tiến độ tải (byte) có tiết lưu nhịp (throttle) trong quá trình chạy `download()`. |

Các trường hợp từ chối cập nhật được trả về dưới dạng kết quả rõ ràng (first-class outcome) chứ không ném ra ngoại lệ (throw error) — các hàm `check`/`download` trả về `status` có thể là `available` / `staged` / `upToDate` / `blacklisted` / `shellTooOld` / `alreadyStaged`, mỗi trạng thái đi kèm ngữ cảnh tương ứng. Mọi cấu trúc dữ liệu truyền qua IPC đều được bảo đảm bằng các bài kiểm thử golden serde.

## Phát hành một gói bundle

### Thao tác một lần: Tạo cặp khóa ký số

Sử dụng công cụ [`minisign`](https://jedisct1.github.io/minisign/) (hoặc `rsign2`) một lần duy nhất, và lưu trữ khóa bí mật tránh xa môi trường CI nếu có thể:

```bash
minisign -G -p hot-update.pub -s hot-update.key
```

Sao chép nội dung tệp `hot-update.pub` (hoặc chỉ dòng bắt đầu bằng `RW…`) dán vào mảng `pubkeys` trong tệp `tauri.conf.json`.

### Thao tác mỗi đợt phát hành: Build, Ký và Tải lên

```bash
# 1. Build frontend như bình thường -> tạo ra thư mục dist/
npm run build

# 2. Đóng gói tar + ký số + tạo manifest.json và manifest.json.minisig
hot-update-sign \
  --dist dist \
  --version 1.2.0 \
  --min-shell 1.1.0 \
  --key ~/.keys/hot-update.key \
  --base-url https://updates.example.com \
  --out release/

# 3. Tải toàn bộ thư mục release/* lên máy chủ/CDN được cấu hình trong manifestUrl
#    (bundle-1.2.0.tar.gz, manifest.json, manifest.json.minisig)
```

Mật khẩu khóa bí mật được đọc tự động từ biến môi trường `HOT_UPDATE_KEY_PASSWORD` nếu có (đặt giá trị rỗng đối với khóa không đặt mật khẩu trên CI); nếu không, chương trình sẽ yêu cầu nhập trực tiếp trên terminal.

### Định dạng tệp kê khai (Manifest format)

Lệnh `hot-update-sign` sẽ tạo ra tệp JSON này, được ký chữ ký tách rời vào tệp `manifest.json.minisig`:

```json
{
  "version": "1.2.0",
  "createdAt": "2026-07-09T00:00:00Z",
  "minShellVersion": "1.1.0",
  "archive": {
    "url": "https://updates.example.com/bundle-1.2.0.tar.gz",
    "sha256": "…64 ký tự hex…",
    "size": 4194304
  }
}
```

- `version` phải **lớn hơn nghiêm ngặt (strictly newer)** so với mọi phiên bản mà ứng dụng đã từng ghi nhận (con dấu chống tấn công hạ cấp/phát lại).
- `minShellVersion` dùng để giới hạn gói bundle chỉ áp dụng cho những phiên bản ứng dụng native đủ mới để chạy nó — các phiên bản ứng dụng cũ hơn sẽ giữ nguyên tài nguyên hiện tại cho đến khi nhận được bản cập nhật từ kho ứng dụng. **Hệ quả tất yếu:** frontend của bạn cần kiểm tra sự tồn tại của tính năng native (feature detection) thay vì mặc định cho rằng tính năng luôn có sẵn.

## Hỗ trợ nền tảng

| Nền tảng | Phục vụ tài nguyên | Áp dụng OTA | Cơ chế hoàn tác | Ghi chú |
|---|---|---|---|---|
| iOS 13+ | `tauri://localhost` | Lần khởi động nguội kế | Có | Trạng thái phiên/đăng nhập được giữ nguyên (origin không đổi). |
| Android 7+ (API 24) | `http://tauri.localhost` | Lần khởi động nguội kế | Có | Sử dụng chứng chỉ gốc CA đính kèm — xem ghi chú bên dưới. |
| macOS / Windows / Linux | `tauri://localhost` | Lần khởi động nguội kế | Có | Cơ chế hoán đổi `Assets` đồng nhất trên mọi nền tảng; hỗ trợ desktop hoàn toàn tự nhiên. |

Plugin được viết bằng **100% Rust nguyên bản** — không sử dụng cầu nối Kotlin hay Swift — bởi vì cơ chế hoán đổi assets và các thao tác I/O tệp đều hoạt động đồng nhất trên các nền tảng.

> **Tại sao chúng tôi đóng gói sẵn chứng chỉ CA (Lưu ý Android dành cho lập trình viên Tauri di động).** Tính năng rustls của `reqwest` kéo theo thư viện `rustls-platform-verifier`, mà backend Android của thư viện này lại cần một JNI context — điều mà plugin Tauri không tự thiết lập. Kết quả là quá trình bắt tay TLS **bị treo vô thời hạn** trên Android. Thay vào đó, plugin này cung cấp cho `reqwest` cấu hình rustls sử dụng bộ chứng chỉ đóng gói sẵn [`webpki-roots`](https://crates.io/crates/webpki-roots) (bộ CA của Mozilla), giúp việc xác thực chứng chỉ hoàn toàn độc lập và hoạt động đồng nhất trên mọi hệ điều hành. Nếu bạn tự viết plugin Tauri di động có thực hiện các yêu cầu HTTPS, đây là một cái bẫy kỹ thuật rất đáng lưu tâm.

## Tuân thủ chính sách kho ứng dụng (OTA có được phép trên iOS & Android không?)

Cả hai kho ứng dụng đều cho phép cập nhật OTA đối với **mã thông dịch** (JS/HTML/CSS trong webview) trong những giới hạn nhất định. Plugin này chỉ cập nhật chính xác tầng mã đó — tuyệt đối không bao giờ can thiệp vào mã native. **Nội dung dưới đây là bản tóm tắt, không phải lời khuyên pháp lý; bạn hoàn toàn chịu trách nhiệm về kết quả xét duyệt ứng dụng của mình.**

**Apple — Thỏa thuận Giấy phép Chương trình Nhà phát triển §3.3.1(B)** (nguyên văn):

> "Except as set forth in the next paragraph, an Application may not download or install executable code. Interpreted code may be downloaded to an Application but only so long as such code: (a) does not change the primary purpose of the Application by providing features or functionality that are inconsistent with the intended and advertised purpose of the Application (b) does not bypass signing, sandbox, or other security features of the OS; and (c) for Applications distributed on the App Store, does not create a store or storefront for other Applications."
>
> *(Dịch ý: Trừ các trường hợp nêu ở đoạn sau, Ứng dụng không được tải về hoặc cài đặt mã thực thi. Mã thông dịch có thể được tải về Ứng dụng nhưng chỉ khi mã đó: (a) không làm thay đổi mục đích chính của Ứng dụng bằng cách cung cấp các tính năng hoặc chức năng không phù hợp với mục đích dự kiến và đã được quảng bá của Ứng dụng; (b) không vượt qua chữ ký số, sandbox hoặc các tính năng bảo mật khác của hệ điều hành; và (c) đối với Ứng dụng phân phối trên App Store, không tạo ra một cửa hàng ứng dụng khác.)*

**Google Play — Chính sách về Lạm dụng Thiết bị và Mạng** (nguyên văn):

> "An app distributed via Google Play may not modify, replace, or update itself using any method other than Google Play's update mechanism. Likewise, an app may not download executable code (such as dex, JAR, .so files) from a source other than Google Play. This restriction does not apply to code that runs in a virtual machine or an interpreter where either provides indirect access to Android APIs (such as JavaScript in a webview or browser)."
>
> *(Dịch ý: Ứng dụng phân phối qua Google Play không được sửa đổi, thay thế hoặc tự cập nhật bằng bất kỳ phương thức nào khác ngoài cơ chế cập nhật của Google Play. Tương tự, ứng dụng không được tải mã thực thi (như tệp dex, JAR, .so) từ nguồn khác ngoài Google Play. Hạn chế này không áp dụng cho mã chạy trong máy ảo hoặc trình thông dịch cung cấp quyền truy cập gián tiếp vào các API của Android (chẳng hạn như JavaScript trong webview hoặc trình duyệt).)*

**Nguyên tắc kỷ luật giúp bạn luôn nằm trong vùng an toàn của cả hai chính sách:** Hãy sử dụng OTA cho việc tinh chỉnh giao diện người dùng, vá lỗi và nội dung — tuyệt đối không thay đổi bản chất chức năng của ứng dụng. Hãy phát hành các thay đổi lớn về tính năng (và bất kỳ thay đổi nào liên quan đến native/Rust) thông qua bản phát hành chính thức trên kho ứng dụng, ngay cả khi về mặt kỹ thuật chúng có thể đẩy qua OTA được. Cả hai chính sách đều không bật đèn xanh vô điều kiện; cả hai đều giữ nguyên điều khoản "không được làm thay đổi chức năng nền tảng của ứng dụng".

## Bảo mật

Cột mốc xác thực tin cậy (trust anchor) chính là **chữ ký số trên tệp kê khai (manifest)**, được xác thực trước khi bất kỳ byte nào của tệp nén được tải về. Việc truyền tải sử dụng giao thức HTTPS, nhưng tính an toàn của hệ thống không phụ thuộc vào HTTPS.

- **Ký số.** Chữ ký minisign được xác thực trực tiếp trên **chuỗi byte thô của manifest** dựa theo danh sách khóa tin cậy, *sau đó* các byte mới được parse thành cấu trúc dữ liệu. Không có bất kỳ đoạn mã xử lý nào phía sau tiếp xúc với dữ liệu chưa được xác minh.
- **Luân chuyển khóa (Key rotation).** Ứng dụng nhúng sẵn một **danh sách** các khóa công khai đáng tin cậy. Quá trình luân chuyển khóa = một bản phát hành qua kho ứng dụng để bổ sung khóa mới; các tệp manifest có thể ký bằng một trong hai khóa trong giai đoạn chuyển đổi; khóa cũ sẽ được loại bỏ trong bản phát hành sau. Không có thời gian chết (downtime), không ép buộc người dùng cập nhật.
- **Chống hạ cấp / phát lại (Downgrade / replay protection).** Dấu mốc `maxVersionSeen` tăng đơn điệu sẽ từ chối bất kỳ manifest nào có phiên bản không lớn hơn phiên bản hiện tại — ngay cả khi manifest đó mang chữ ký hoàn toàn hợp lệ — ngăn chặn kẻ tấn công MITM hoặc bộ nhớ đệm cũ phát lại gói cập nhật cũ chứa lỗ hổng bảo mật. Bản phát hành mới trên kho ứng dụng có tài nguyên nhúng mới hơn cũng sẽ tự động hủy bỏ gói OTA cũ.
- **Tính toàn vẹn.** Tệp nén được kiểm tra đối chiếu mã băm `sha256` và kích thước `size` chính xác theo manifest trước khi tiến hành giải nén.
- **Giải nén an toàn được gia cố.** Tệp tar.gz được giải nén với cơ chế từ chối liên kết mềm (symlink) / liên kết cứng (hardlink), phòng chống duyệt đường dẫn độc hại (zip-slip) và giới hạn kích thước sau giải nén (phòng chống zip-bomb).
- **Không có nguồn cập nhật do JS kiểm soát.** URL manifest và danh sách khóa chỉ đến từ tệp cấu hình `tauri.conf.json`, được đóng gói cố định vào mã nhị phân có chữ ký số của kho ứng dụng — mã độc trên webview không thể chuyển hướng nguồn cập nhật sang máy chủ khác.

**Những gì mô hình tin cậy *không* bao gồm:** Việc để lộ khóa ký bí mật (cần luân chuyển khóa ngay lập tức và phát hành bản cập nhật kho ứng dụng để loại bỏ khóa cũ), và tính toàn vẹn của mã native (mã native chỉ thay đổi qua bản phát hành kho ứng dụng, được hệ điều hành ký số bảo vệ).

## So sánh với các giải pháp khác

So sánh khách quan và dựa trên dữ kiện thực tế. Expo Updates và CodePush được liệt kê dưới dạng mô hình tham chiếu ý tưởng — chúng dành cho React Native, **không** áp dụng cho Tauri.

| Dự án | Framework | Tauri v2 | Mobile (iOS + Android) | Tự lưu trữ (Self-hosted) | Ký số (Signing) | Hoàn tác (Rollback) | Giấy phép / Ghi chú |
|---|---|---|---|---|---|---|---|
| **tauri-plugin-hot-update** (plugin này) | Tauri | ✅ | ✅ Cả hai | ✅ Bất kỳ host tĩnh / CDN nào | ✅ minisign, luân chuyển danh sách khóa | ✅ 3 trạng thái, danh sách đen mã băm, chống hạ cấp/phát lại | MIT/Apache-2.0 |
| [`denniskribl/tauri-plugin-hotswap`](https://github.com/denniskribl/tauri-plugin-hotswap) | Tauri | ✅ | ✅ Tuyên bố hỗ trợ (README) | ✅ | ✅ minisign | ✅ Dựa trên tín hiệu xác nhận ack | MIT; một người duy trì, CI chỉ chạy Linux/macOS/Windows (không có CI di động). Cùng kiến trúc hoán đổi `Assets` — ý tưởng hội tụ độc lập. |
| [`@capgo/tauri-updater`](https://github.com/Cap-go/tauri-updater) | Tauri | ✅ | ⚠️ Chưa rõ; mô phỏng theo Capacitor | ✅ hoặc Capgo cloud | Qua các plugin kết hợp | Qua các plugin kết hợp | Điều phối bằng TypeScript thuần trên các plugin chính thức fs/http/process — **không có mã plugin Rust native**; khả năng chuyển hướng tải tài nguyên trên webview di động chưa được kiểm chứng. |
| CrabNebula OTA (`tauri-plugin-ota-updater`) | Tauri | ✅ | ⚠️ Không đề cập rõ ràng cho di động | ❌ Phụ thuộc đám mây (CrabNebula Cloud) | ✅ (đám mây) | ✅ | PolyForm-Noncommercial — **không hoàn toàn là mã nguồn mở (OSS)**; gói dịch vụ trả phí/đám mây. |
| [Expo Updates](https://docs.expo.dev/versions/latest/sdk/updates/) | **React Native** | ❌ | ✅ | ✅ hoặc EAS | ✅ Ký mã nguồn | ✅ | Mô hình tham chiếu của React Native; không áp dụng cho Tauri. |
| [Microsoft CodePush](https://learn.microsoft.com/en-us/appcenter/distribution/codepush/) | RN / Cordova | ❌ | ✅ | ❌ Do Microsoft lưu trữ | ✅ | ✅ | App Center CodePush **ngừng hoạt động từ tháng 3/2025**; plugin này đưa *mô hình kiến trúc* đó lên Tauri. |

## Giấy phép bản quyền

Được cấp phép kép theo một trong hai giấy phép [Apache License, Version 2.0](LICENSE-APACHE) hoặc [MIT License](LICENSE-MIT) tùy bạn lựa chọn.

Trừ khi bạn có tuyên bố rõ ràng khác, bất kỳ đóng góp nào bạn chủ ý gửi để đưa vào dự án này, theo định nghĩa trong giấy phép Apache-2.0, sẽ được cấp phép kép như trên mà không kèm theo bất kỳ điều khoản hoặc điều kiện bổ sung nào.
