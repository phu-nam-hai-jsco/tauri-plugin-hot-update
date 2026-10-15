# tauri-plugin-hot-update-api

[English](README.md) | [Tiếng Việt](README.vi.md)

Thư viện TypeScript bindings cho [`tauri-plugin-hot-update`](https://github.com/yanqianglu/tauri-plugin-hot-update) — giải pháp cập nhật nóng / cập nhật trực tiếp qua mạng OTA (theo phong cách CodePush, tự lưu trữ - self-hosted) cho ứng dụng di động và máy tính để bàn (desktop) chạy Tauri v2. Phát hành các bản cập nhật gói frontend (bundle) đến các ứng dụng đã cài đặt mà không cần thông qua đợt xét duyệt của kho ứng dụng, được bảo vệ bởi máy trạng thái (state machine) tự động hoàn tác chống brick (chống hỏng ứng dụng).

Yêu cầu plugin Rust đã được cài đặt và cấu hình (URL tệp manifest + khóa công khai minisign trong `tauri.conf.json`) — xem [README của plugin chính](../README.vi.md) để biết cách thiết lập, ký số và phát hành.

## Cách sử dụng

```ts
import {
  check,
  download,
  notifyAppReady,
  currentBundle,
  onDownloadProgress,
} from "tauri-plugin-hot-update-api";

// 1. Trong mỗi lần khởi chạy, ngay sau khi giao diện shell của ứng dụng đã render xong:
//    xác nhận (commit) gói đang chạy là bản hoạt động tốt gần nhất (last-known-good).
//    Lần khởi chạy nào không bao giờ gửi xác nhận sẽ bị hoàn tác (rollback) ở lần khởi động kế tiếp.
//    An toàn khi gọi vô điều kiện.
await notifyAppReady();

// 2. Bất cứ khi nào ứng dụng quyết định kiểm tra (khi khởi chạy/tiếp tục - plugin không bao giờ tự động thăm dò):
const unlisten = await onDownloadProgress(({ downloaded, total }) => {
  console.log(`Tải xuống OTA: ${Math.round((downloaded / total) * 100)}%`);
});
const outcome = await download();
unlisten();

if (outcome.status === "staged") {
  // Bản cập nhật sẽ được áp dụng trong lần khởi động nguội (cold launch) tiếp theo.
  console.log(`v${outcome.version} đã sẵn sàng — hãy khởi động lại để áp dụng`);
}

// Kiểm tra thông tin gói hiện tại:
const bundle = await currentBundle();
// { source: "ota" | "embedded", seq: number | null, version: string }
```

Hàm `check()` tìm nạp và xác thực tệp manifest mà không tải gói nén về, phù hợp cho các ứng dụng muốn hỏi ý kiến người dùng trước khi tải bản cập nhật.

## Quyền hạn (Permissions)

Cấp quyền cho các lệnh của plugin trong tệp cấu hình năng lực (capability file):

```json
{ "permissions": ["hot-update:default"] }
```

Giấy phép kép theo MIT hoặc Apache-2.0 tùy chọn.
