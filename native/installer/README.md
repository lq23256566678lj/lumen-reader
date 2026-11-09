# Windows 安装包

安装包由 NSIS 3.13 编译，脚本为 `lumen-reader.nsi`。

## 安装行为

- 按用户安装，不需要管理员权限。
- 默认安装目录：`%LOCALAPPDATA%\Programs\LumenReader`
- 创建开始菜单快捷方式和桌面快捷方式。
- 注册卸载项，可从“设置 → 应用”卸载。
- 安装内容包含 exe、`pdfium\pdfium.dll`、示例书、README 和使用说明。
- 卸载只删除程序文件；`%APPDATA%\LumenReader` 中的书架、进度、书签和云配置会保留。

## 重新编译

先准备 NSIS（`makensis`），然后在当前目录执行：

```powershell
makensis /V3 lumen-reader.nsi
```

脚本默认读取 `..\..\lumen-reader-dist`，输出到
`..\..\lumen-reader-setup-x64.exe`。

`lumen.ico` 是安装程序图标。
