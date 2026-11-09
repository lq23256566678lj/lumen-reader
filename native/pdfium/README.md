# PDFium runtime

`pdfium-render` is a runtime binding; it does not compile PDFium into this
project. The Windows x64 build in this folder comes from
[bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) and is
distributed under PDFium's BSD-3-Clause license.

The reader looks for the library in this order:

1. `PDFIUM_DYNAMIC_LIB_PATH`
2. the executable's directory and its `pdfium/` subdirectory
3. `./pdfium`
4. `<project>/pdfium`
5. the system library path

On another platform, replace `pdfium.dll` with the appropriate
`libpdfium.so` / `libpdfium.dylib`. You can re-fetch the Windows build with:

```powershell
pwsh -File scripts/fetch-pdfium.ps1
```

If the library is missing, non-PDF formats still work; opening a PDF shows a
clear "PDFium 未找到" message.
