# Lumen Reader

Lumen Reader（微光阅读）是一个本地优先的电子书阅读器套件，包含两个独立实现：

| 版本 | 技术 | 适用场景 |
| --- | --- | --- |
| **Native** | Rust + egui/eframe | 轻量原生窗口，完整的书架、导入、同步和多格式阅读 |
| **WebView** | Rust + wry/tao + 系统 WebView | 用浏览器内核渲染 EPUB，保留彩色页、固定版式和原书 CSS |

两个程序可以同时安装，共用 `LumenReader` 数据目录，互不覆盖。

## 功能

- EPUB / MOBI / AZW / AZW3 / PDF / FB2 / CBZ / TXT
- 书架、文件夹、搜索、封面、导入
- 阅读进度、书签、字号/行距/主题
- 智能卷序排序（`上/中/下`、`第一卷/第二卷`、`1/2/10`）
- WebDAV 同步（进度、书签、文件夹）
- EPUB 彩页按卷首合并
- 图片按正文位置显示（Native 版）

## 平台

| 平台 | Native | WebView |
| --- | --- | --- |
| Windows 10/11 | ✅ | ✅ WebView2 Runtime |
| Linux | ✅ GTK/GL | ✅ WebKitGTK |
| macOS | ✅ | ✅ WKWebView |

Windows 安装包和 portable 包在 [Releases](../../releases) 提供。Linux/macOS 目前从源码构建。

### Native 构建

```bash
cd native
cargo build --release
```

Linux 需要 GTK/GL 开发包，macOS 需要 Xcode Command Line Tools。PDF 需要动态 `pdfium` 库：把它放在可执行文件旁的 `pdfium/`，或设置 `PDFIUM_DYNAMIC_LIB_PATH`。

### WebView 构建

```bash
cd webview
cargo build --release
```

Linux 需要 `libwebkit2gtk-4.1-dev`、`libgtk-3-dev` 等；Windows 需要 WebView2 Runtime；macOS 使用系统 WKWebView。

详细依赖见 [docs/PLATFORMS.md](docs/PLATFORMS.md)。

## 数据目录

- Windows：`%APPDATA%\LumenReader`
- Linux：`$XDG_DATA_HOME/LumenReader` 或 `~/.local/share/LumenReader`
- macOS：`~/Library/Application Support/LumenReader`

可用 `LUMEN_DATA_DIR` 覆盖。两个程序共用 `library.json`、书签和 WebDAV 配置。

## 开发

```bash
# 原生版
cargo run --release --manifest-path native/Cargo.toml

# WebView 版
cargo run --release --manifest-path webview/Cargo.toml
```

## License

MIT
