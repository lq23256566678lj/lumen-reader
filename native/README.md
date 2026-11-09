# Lumen Reader · 微光阅读

一个用 Rust 编写的本地优先电子书阅读器。界面基于 `egui`/`eframe`，支持
EPUB、MOBI、AZW/AZW3、PDF、FB2、CBZ、TXT，使用统一的 `BookSource` 抽象把
“格式解析”和“界面渲染”彻底分开。

> 设计目标：书架好整理、阅读区舒服、翻页/滚动都不掉帧；大书只加载当前章节；
> 云同步只传进度和书签，不碰书文件。

## 功能

- **多格式**：EPUB、MOBI、AZW、AZW3、PDF、FB2、CBZ、TXT。
- **DRM 拒绝**：检测到 Adobe ADEPT、Readium LCP、Mobipocket/Kindle DRM、
  加密 PDF 或内容加密的 EPUB 时，直接给出中文提示，不尝试绕过。
- **书架**：批量导入文件/文件夹、封面网格、搜索、自定义文件夹、阅读进度、
  导入进度和逐条失败原因。
- **智能排序**：识别 `上/中/下`、`第1部/第2部`、`1/2/10` 等分卷写法，把同一
  系列按卷序排在一起；新建文件夹后可在书籍右键菜单中归类。
- **阅读**：目录树、章节跳转、滚动/翻页两种模式、字号调节、亮/暗主题、
  快捷键、阅读位置记忆、书签。
- **章节整理**：按 `第X章 / 第X节 / Chapter N` 等标题重新分段，保留封面、
  彩页、目录、简介、制作信息等前置页面，合并只有几句话的正文碎片，并把超大
  文件按段落拆到合适长度。
- **插图**：提取 EPUB/MOBI 章节内联的 base64 插图并显示在正文上方；
  彩色页面整合到同一个「彩页」章节，正文内联颜色尽量保留；CSS 背景图也会提取。
- **打开书籍**：章节整理在后台线程执行，书架保持响应并显示打开进度。
- **大文件友好**：EPUB 使用 `ebook-rs` 的 lazy archive；TXT 按约 24k 字符
  切分虚拟章节；UI 一次只加载并排版当前章节。
- **云同步**：WebDAV 同步阅读进度、书签和书架元数据，使用 `egui-async`
  在后台任务中执行，不阻塞 UI；冲突按 `updated_at` 最后写入者优先，书签按 ID
  合并并支持删除墓碑。
- **系统字体**：运行时从系统字体目录加载，不把字体打包进二进制。

## 快速开始

### 1. 环境

- Rust **1.95+**（`edition = "2024"`）
- Windows / macOS / Linux 桌面环境
- Windows 构建需要 MSVC 工具链（本仓库在 `x86_64-pc-windows-msvc` 上验证）

### 2. 运行

```bash
cd lumen-reader
cargo run --release
```

首次启动会在 `dirs::data_dir()/LumenReader/` 创建：

```text
library.json     # 书架、阅读进度、书签
settings.json    # 字号、主题、翻页模式等
cloud.json       # WebDAV 配置（含本地保存的密码，见下方安全说明）
covers/          # 封面 PNG 缩略图（仅本地，不参与云同步）
```

开发时可以用 `LUMEN_DATA_DIR` 指定一个便携数据目录：

```powershell
$env:LUMEN_DATA_DIR = "D:\LumenData"
cargo run --release
```

### 3. PDFium

`pdfium-render` 在运行时动态绑定 PDFium，项目不把 PDFium 编译进二进制。
Windows x64 的 `pdfium.dll` 已放在 `pdfium/` 目录中；其他平台请替换为对应的
`libpdfium.so` / `libpdfium.dylib`。也可以重新下载：

```powershell
pwsh -File scripts/fetch-pdfium.ps1
```

查找顺序：`PDFIUM_DYNAMIC_LIB_PATH` → 程序目录/`pdfium` → 当前目录/`pdfium`
→ 项目目录/`pdfium` → 系统库。PDFium 缺失时其他格式仍可正常使用，打开 PDF
会显示明确提示。

## 项目结构

```text
lumen-reader/
├─ Cargo.toml              # 所有直接依赖固定版本
├─ Cargo.lock
├─ pdfium/                 # 动态库 + 说明
├─ samples/                # 生成的 EPUB/TXT/CBZ/PDF/FB2 示例
├─ scripts/fetch-pdfium.ps1
├─ src/
│  ├─ main.rs              # 启动、窗口、eframe 入口
│  ├─ lib.rs               # 模块导出、lint 策略
│  ├─ app.rs               # App 状态、事件循环、导入/同步任务调度
│  ├─ parser/
│  │  ├─ mod.rs            # 格式探测、open_book、封面提取
│  │  ├─ book_source.rs    # trait BookSource
│  │  ├─ ebook.rs          # ebook-rs 适配（EPUB/MOBI/AZW3/FB2/TXT/CBZ）
│  │  ├─ pdf.rs            # pdfium-render 适配 + PDFium 绑定
│  │  └─ drm.rs            # DRM/加密检测
│  ├─ state/
│  │  ├─ models.rs         # serde 数据模型
│  │  ├─ storage.rs        # dirs::data_dir、JSON 原子写入、id、封面
│  │  ├─ import.rs         # 后台批量导入线程
│  │  └─ reader.rs         # 当前章节、进度、分页、书签
│  ├─ cloud/
│  │  ├─ config.rs         # WebDAV 配置、设备 ID、密码环境变量
│  │  ├─ webdav.rs         # GET/PUT/MKCOL/PROPFIND 客户端
│  │  └─ sync.rs           # 快照、合并、冲突策略
│  └─ ui/
│     ├─ theme.rs          # modern-egui + 阅读器配色
│     ├─ fonts.rs          # 系统字体运行时加载
│     ├─ widgets.rs        # 居中、封面占位、胶囊、格式化
│     ├─ library.rs        # 书架
│     ├─ reader.rs         # 阅读区、目录、翻页
│     └─ settings.rs       # 设置、WebDAV、快捷键、对话框、Toast
└─ examples/
   ├─ inspect.rs           # 命令行检查任意书籍解析/渲染
   └─ cloud_sync.rs        # 本地 WebDAV 端到端联调示例
```

## 核心抽象

`parser/book_source.rs` 定义了 UI 唯一依赖的阅读接口：

```rust
pub trait BookSource: Send + Sync {
    fn metadata(&self) -> &BookMetadata;
    fn toc(&self) -> &[TocEntry];
    fn chapter_count(&self) -> usize;
    fn chapter_content(&self, index: usize) -> Result<ChapterBody, BookError>;

    fn is_paged(&self) -> bool { false }
    fn cover(&self) -> Option<&[u8]> { None }
    fn render_page(&self, index: usize, max_width: u32)
        -> Result<Option<RenderedPage>, BookError> { Ok(None) }
}
```

- `ChapterBody::Text` 用于重排格式；
- `ChapterBody::Page` + `render_page` 用于 PDF/CBZ；
- UI 只判断 `is_paged()`，不匹配任何具体格式。

### 懒加载策略

1. EPUB 使用 `EpubArchive::open` + `Book::from_archive_lazy`，章节按需
   `get_section`；
2. TXT 读取后按 24k 字符切分成虚拟章节，避免把整本书作为一个巨大 label；
3. `ReaderSession` 只缓存当前章节文本，切章时重新加载；
4. 翻页模式只对当前章节做分页，不预排版整本书；
5. PDF/CBZ 只渲染当前页，页面纹理缓存最多保留 4 张，宽度上限 1800px。

## 依赖版本

| 依赖 | 版本 | 用途 |
| --- | --- | --- |
| `eframe` / `egui` | `=0.36.2` | GUI |
| `egui-async` | `=0.7.1` | WebDAV 异步任务 |
| `egui-file-dialog` | `=0.15.0` | 文件/文件夹选择 |
| `modern-egui` | `=0.2.3` | 统一主题与样式 |
| `ebook-rs` | `=0.18.0` | 多格式解析引擎 |
| `pdfium-render` | `=0.9.5` | PDF 渲染 |
| `image` | `=0.25.10` | 封面/漫画页解码 |
| `zip` | `=9.2.2` | EPUB DRM 容器检查 |
| `reqwest` | `=0.13.5` | WebDAV 客户端（rustls） |
| `serde` / `serde_json` | `=1.0.229` / `=1.0.154` | JSON 持久化 |
| `dirs` | `=6.0.0` | 系统数据目录 |
| `thiserror` | `=2.0.21` | 分层错误类型 |

完整版本以 `Cargo.toml` / `Cargo.lock` 为准。

## WebDAV 云同步

在 **云同步** 窗口中填写：

```text
服务器地址：https://dav.example.com/remote.php/dav/files/me/
用户名：me
密码：••••••
远端目录：books/lumen
```

同步流程：

1. `MKCOL` 逐级创建远端目录；
2. `GET lumen-reader-sync.json`（不存在则视为首次上传）；
3. 本地/远端快照合并；
4. `PUT` 合并后的 JSON。

快照只包含：

```json
{
  "schema": 1,
  "device_id": "device-...",
  "updated_at": 1790000000,
  "books": [
    { "id": "内容指纹", "title": "书名", "author": "作者",
      "format": "epub", "chapter_count": 12, "progress": { "chapter": 3, "fraction": 0.42 } }
  ],
  "bookmarks": [
    { "id": "...", "book_id": "内容指纹", "chapter": 3, "fraction": 0.42,
      "label": "第三章 · 42%", "deleted": false }
  ]
}
```

**不会上传**：书文件、封面、绝对路径、本地设置。书籍身份使用“文件大小 +
首尾 64KB 内容”的稳定指纹，所以书被移动或换机器后仍能匹配进度。

冲突策略：

- 每本书进度比较 `progress.updated_at`，较新者胜；
- 书签按 `id` 合并，删除通过 `deleted + updated_at` 墓碑同步；
- 远端独有书目会以“云端书目”占位符出现在书架，导入相同文件后自动补全。

### 密码安全

`cloud.json` 默认会保存密码明文（本机数据目录）。如果不希望落盘，设置：

```powershell
$env:LUMEN_WEBDAV_PASSWORD = "your-app-password"
```

此时配置中的密码字段会被忽略。生产使用时建议使用 WebDAV 应用专用密码。

## 快捷键

| 按键 | 操作 |
| --- | --- |
| `Esc` | 返回书架 / 关闭弹窗 |
| `←` / `→` | 上一页 / 下一页 |
| `PageUp` / `PageDown` | 上一页 / 下一页 |
| `Space` | 翻页模式下下一页 |
| `+` / `-` | 放大 / 缩小字号 |
| `T` | 显示/隐藏目录 |
| `V` | 切换滚动/翻页 |
| `D` | 亮/暗主题 |
| `B` | 添加书签 |
| `Ctrl+O` | 导入电子书 |
| `Ctrl+S` | 立即云同步 |

文本框聚焦时快捷键自动让位给输入。

## 开发与测试

```bash
cargo test
cargo clippy --all-targets
cargo run --example inspect -- samples/sample.epub
```

当前单元测试覆盖：

- 分页区间连续、覆盖全文、空文本；
- EPUB 字体混淆白名单 vs 内容加密拒绝；
- MOBI PalmDOC 加密字段检测；
- 内容指纹不受路径影响；
- 云快照冲突合并、云端占位书目；
- TXT 经过统一 `BookSource` 打开。

## 约束与实现说明

- **禁止 `unwrap()` / `expect()`**：源码中不出现这两个调用；错误统一通过
  `Result`、`thiserror`、`?` 和 `map_or_else` 处理。`lib.rs`/`main.rs` 还启用了
  `clippy::unwrap_used`、`clippy::expect_used` 的 deny。
- **系统字体**：`eframe` 关闭 `default_fonts`，启动时从 `C:\Windows\Fonts`、
  macOS 字体目录或 Linux 常见字体目录读取 TTF/TTC/OTF。找不到字体时给出明确
  错误，而不是打包备用字体。
- **DRM**：EPUB 的 `META-INF/encryption.xml` 只放行字体混淆（IDPF/Adobe
  font obfuscation）；`rights.xml`、LCP license、非字体加密、加密 PDF、
  MOBI/AZW 的非零 encryption type 均拒绝。
- **阅读区**：正文最大宽度默认 760px，窗口宽时居中留白；字号、宽度、主题
  可实时调整。
- **封面**：导入时写入本地 `covers/<id>.png`，不参与云同步，避免把二进制
  元数据塞进 JSON。

## 已知限制

- CBR 未列入支持格式；CBZ 支持 ZIP 漫画。
- MOBI/AZW3 的解析能力取决于 `ebook-rs`；部分 HUF/CDIC 压缩的 MOBI 会被
  解析器明确拒绝，并提示转换。
- PDF 依赖外部 PDFium；未放置动态库时 PDF 不可读。
- 云同步是最后写入者优先，适合个人多设备使用，不是协作编辑协议。
- 封面和窗口截图只保存在本地。

## License

MIT OR Apache-2.0。第三方依赖及 PDFium 的许可证见各自仓库/发行包。
