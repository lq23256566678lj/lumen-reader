# Lumen Reader · WebView2

独立的 **WebView2 电子书阅读器**。书架、文件夹、进度、书签、阅读设置和 WebDAV 同步都在这个版本里完成。

## 支持

- **EPUB**：原始 XHTML/CSS 渲染，保留彩页、固定版式、圆圈、人物名、SVG 和背景图。
- **PDF**：调用 WebView2 内置 PDF 查看器。
- **TXT / FB2 / CBZ / MOBI / AZW / AZW3**：转换或解析为可滚动正文。
- 书架读取 `%APPDATA%\LumenReader\library.json`，沿用旧版的书籍、文件夹和封面。
- 导入电子书后写入书架。
- 阅读进度自动保存，再次打开时回到上次章节和位置。
- 书签：添加、跳转、删除。
- 阅读设置：字号、行距、深色/羊皮纸/浅色主题、正文字体。
- 文件夹：新建、重命名、删除；右键书籍移动到文件夹。
- 智能卷序排序：`上/中/下`、`第1卷/第2卷`、`1/2/10` 按卷序排列。
- WebDAV 同步：进度、书签、文件夹和书目合并后上传。

## 使用

双击桌面图标，或直接运行：

```text
lumen-reader-webview.exe
lumen-reader-webview.exe "D:\Books\book.epub"
```

阅读界面的“设置”里填写 WebDAV 地址、用户名、密码和远端目录，然后测试连接或立即同步。

## 数据位置

- 书架和进度：`%APPDATA%\LumenReader\library.json`
- WebView2 设置：`%APPDATA%\LumenReader\webview-settings.json`
- WebDAV 配置：`%APPDATA%\LumenReader\webdav.json`
- 程序运行缓存：程序目录下 `WebView2Data`

需要 Windows 10/11 的 **WebView2 Runtime**（大多数系统已自带）。

## 技术结构

- Rust + `wry` / `tao` 创建 WebView2 窗口。
- 内置本地 HTTP 服务提供书架、书籍资源、进度、书签、文件夹和 WebDAV 同步接口。
- 前端 HTML/CSS/JS 负责界面；EPUB 原文交给 WebView2 的浏览器内核渲染。
