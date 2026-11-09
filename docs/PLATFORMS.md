# Platform build notes

## Windows

- Rust stable MSVC toolchain.
- WebView 版需要 Microsoft Edge WebView2 Runtime（Windows 10/11 通常已预装）。
- 安装包由 NSIS 构建；图标资源在 Windows 构建时通过 `winres` 嵌入。

## Linux

Native（egui/glow）：

```bash
sudo apt install build-essential pkg-config libgtk-3-dev libxkbcommon-dev \
  libwayland-dev libx11-dev libxi-dev libgl1-mesa-dev libasound2-dev libssl-dev
cargo build --release --manifest-path native/Cargo.toml
```

WebView（wry/WebKitGTK）：

```bash
sudo apt install build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev librsvg2-dev libssl-dev
cargo build --release --manifest-path webview/Cargo.toml
```

Wayland/X11 通过 winit 自动选择。

PDF 支持需要系统安装 `pdfium` 动态库，或设置：

```bash
export PDFIUM_DYNAMIC_LIB_PATH=/path/to/pdfium
```

## macOS

- Xcode Command Line Tools。
- Native 使用 eframe/glow；WebView 使用系统 WKWebView。
- PDF 需要把 `libpdfium.dylib` 放到可执行文件旁或设置 `PDFIUM_DYNAMIC_LIB_PATH`。

## WebView 名称说明

“WebView2”是 Windows 上的微软内核名称。代码通过 `wry` 使用各平台系统 WebView：

- Windows：WebView2
- Linux：WebKitGTK
- macOS：WKWebView

因此同一个 `webview` 源码可以在三端构建。
