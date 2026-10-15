# Danh sách kiểm tra phát hành (Publishing checklist)

[English](PUBLISHING.md) | [Tiếng Việt](PUBLISHING.vi.md)

Các bước chính xác để đưa `tauri-plugin-hot-update` từ kho mã nguồn này lên crates.io, npm và GitHub. Hãy thực hiện theo đúng thứ tự. Không có bước nào ở đây được tự động hóa — không có quy trình CI nào tự động phát hành khi push code.

## Tài khoản / Thông tin xác thực cần thiết

- **crates.io** — đã đăng nhập với tư cách chủ sở hữu crate (`cargo login <token>`); đây là lần phát hành đầu tiên của tên `tauri-plugin-hot-update`, do đó tên này phải khả dụng.
- **npm** — đã đăng nhập với tư cách chủ sở hữu của gói `tauri-plugin-hot-update-api` (`npm whoami` thành công); lần đầu phát hành tên gói này.
- **GitHub** — lệnh `gh auth status` báo xanh, có quyền tạo repo dưới tài khoản `yanqianglu`.
- **Khóa bí mật minisign** — chỉ cần thiết khi ký các bản phát hành OTA thực tế, không cần thiết khi phát hành plugin. Không sử dụng tại đây.

## 0. Kiểm tra tiền phát hành (Tất cả bắt buộc phải vượt qua)

```bash
# Rust: định dạng mã, kiểm tra lint, toàn bộ bộ test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

# Biên dịch CLI ký phát hành theo feature của nó
cargo build --features cli --bin hot-update-sign

# Biên dịch các mục tiêu di động (mục đích cốt lõi của plugin)
rustup target add aarch64-apple-ios aarch64-linux-android
cargo check --target aarch64-apple-ios
cargo check --target aarch64-linux-android

# Kiểm tra tính hợp lệ của crate đã đóng gói & chứa đúng nội dung mong muốn (chưa phát hành qua mạng)
cargo publish --dry-run
cargo package --list      # kiểm tra bằng mắt danh sách file

# Xây dựng & kiểm tra kiểu gói TypeScript
cd guest-js
npm install
npm run typecheck
npm run build             # tạo thư mục dist/ (đã được gitignore; prepublishOnly sẽ chạy lại)
npm pack --dry-run        # kiểm tra nội dung gói nén (dist/ + README.md)
cd ..
```

Lần xác minh gần nhất tại máy cục bộ: `cargo test` → 130 bài test vượt qua; `cargo clippy --all-targets` → sạch lỗi cảnh báo.

## 1. Rà soát siêu dữ liệu (Metadata) lần cuối

- `Cargo.toml`: `version`, `description`, `keywords` (tối đa 5), `categories`, `repository`, `homepage`, `documentation`, `readme`, `license` — tất cả đã được thiết lập đầy đủ.
- `guest-js/package.json`: `version` khớp với crate, `keywords`, `repository`, `license`, `files` (`dist` + `README.md`).
- `README.md` hiển thị đúng trên GitHub (kiểm tra các bảng và mục tiếng Trung, tiếng Việt).
- Tăng số phiên bản ở **cả hai tệp** `Cargo.toml` và `guest-js/package.json` cùng lúc nếu đây không phải bản phát hành đầu tiên `0.1.0`.

## 2. Phát hành crate (crates.io)

```bash
cargo publish
```

- docs.rs sẽ tự động dựng tài liệu từ crate đã phát hành — không cần thao tác thêm; huy hiệu `docs.rs` sẽ chuyển sang hoạt động trong vài phút.
- Tệp nhị phân `hot-update-sign` được phân phối kèm trong crate dưới feature `cli`; người dùng cài đặt thông qua lệnh: `cargo install tauri-plugin-hot-update --features cli`.

## 3. Phát hành gói npm

```bash
cd guest-js
npm publish            # prepublishOnly sẽ chạy `npm run build` trước
cd ..
```

- Gói được công khai và không có scope (`tauri-plugin-hot-update-api`); nếu tính năng 2FA của npm được bật, hãy chuẩn bị sẵn mã OTP.

## 4. Tạo repo GitHub + Push mã nguồn

```bash
gh repo create yanqianglu/tauri-plugin-hot-update \
  --public \
  --description "Hot update / OTA / live update (CodePush-style, self-hosted) for Tauri v2 mobile and desktop apps" \
  --source . --remote origin --push
```

(Hoặc `git remote add origin git@github.com:yanqianglu/tauri-plugin-hot-update.git && git push -u origin main` nếu repo đã tồn tại sẵn.)

## 5. Cài đặt các chủ đề GitHub (Tăng khả năng tìm thấy)

```bash
gh repo edit yanqianglu/tauri-plugin-hot-update \
  --add-topic tauri \
  --add-topic tauri-plugin \
  --add-topic tauri-v2 \
  --add-topic hot-update \
  --add-topic ota \
  --add-topic over-the-air \
  --add-topic live-update \
  --add-topic codepush \
  --add-topic codepush-alternative \
  --add-topic expo-updates \
  --add-topic hotfix \
  --add-topic self-hosted \
  --add-topic ios \
  --add-topic android \
  --add-topic mobile \
  --add-topic rust
```

## 6. Gắn thẻ phiên bản phát hành (Tag release)

```bash
git tag v0.1.0
git push origin v0.1.0
gh release create v0.1.0 --generate-notes
```

## Kiểm tra sau khi phát hành (Post-publish sanity)

- Chạy thử `cargo add tauri-plugin-hot-update` trong một dự án tạm để kiểm tra tải gói thành công.
- Chạy `npm view tauri-plugin-hot-update-api` để kiểm tra phiên bản và từ khóa hiển thị đúng.
- Các huy hiệu crates.io / npm / docs.rs trong tệp README đều hiển thị màu xanh lá cây.
- Tìm kiếm "tauri hot update", "tauri codepush", "tauri 热更新" trên GitHub/crates.io sau khi hệ thống lập chỉ mục — repo/crate cần phải xuất hiện trong kết quả tìm kiếm.
