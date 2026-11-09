Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
CRCCheck on
XPStyle on

!include "MUI2.nsh"

!define APP_NAME "Lumen Reader WebView2"
!define APP_VERSION "0.2.6"
!define ICON "..\..\lumen-reader\installer\lumen.ico"
!define APP_EXE "lumen-reader-webview.exe"
!define APP_REG "Software\LumenReaderWebView"
!define APP_UNINSTALL "Software\Microsoft\Windows\CurrentVersion\Uninstall\LumenReaderWebView"
!define DIST "..\..\lumen-reader-webview-dist"

Name "${APP_NAME}"
Icon "${ICON}"
UninstallIcon "${ICON}"
OutFile "..\..\lumen-reader-webview-setup-x64.exe"
InstallDir "$LOCALAPPDATA\Programs\LumenReaderWebView"
InstallDirRegKey HKCU "${APP_REG}" "InstallDir"
BrandingText "${APP_NAME} ${APP_VERSION}"
VIProductVersion "0.2.6.0"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"
VIAddVersionKey "FileDescription" "Lumen Reader WebView2 安装程序"
VIAddVersionKey "CompanyName" "Lumen Reader"
VIAddVersionKey "LegalCopyright" "MIT OR Apache-2.0"
VIAddVersionKey "FileVersion" "${APP_VERSION}.0"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "Install"
    SetOutPath "$INSTDIR"
    RMDir /r "$INSTDIR\WebView2Data"
    RMDir /r "$INSTDIR\WebView2Data.old"
    File "${DIST}\${APP_EXE}"
    File "${DIST}\README.md"
    File "${DIST}\lumen.ico"
    WriteUninstaller "$INSTDIR\Uninstall.exe"

    CreateDirectory "$SMPROGRAMS\Lumen Reader WebView2"
    CreateShortcut "$SMPROGRAMS\Lumen Reader WebView2\Lumen Reader WebView2.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
    CreateShortcut "$DESKTOP\Lumen Reader WebView2.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0

    WriteRegStr HKCU "${APP_REG}" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "${APP_REG}" "Version" "${APP_VERSION}"
    WriteRegStr HKCU "${APP_UNINSTALL}" "DisplayName" "${APP_NAME}"
    WriteRegStr HKCU "${APP_UNINSTALL}" "DisplayVersion" "${APP_VERSION}"
    WriteRegStr HKCU "${APP_UNINSTALL}" "Publisher" "Lumen Reader"
    WriteRegStr HKCU "${APP_UNINSTALL}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${APP_UNINSTALL}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegDWORD HKCU "${APP_UNINSTALL}" "NoModify" 1
    WriteRegDWORD HKCU "${APP_UNINSTALL}" "NoRepair" 1
SectionEnd

Section "Uninstall"
    Delete "$DESKTOP\Lumen Reader WebView2.lnk"
    RMDir /r "$SMPROGRAMS\Lumen Reader WebView2"
    DeleteRegKey HKCU "${APP_UNINSTALL}"
    DeleteRegKey HKCU "${APP_REG}"
    RMDir /r "$INSTDIR"
SectionEnd
