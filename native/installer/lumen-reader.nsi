Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
CRCCheck on
XPStyle on

!include "MUI2.nsh"

!define APP_NAME "Lumen Reader"
!define APP_NAME_CN "微光阅读"
!define APP_VERSION "0.2.9"
!define APP_PUBLISHER "Lumen Reader"
!define APP_EXE "lumen-reader.exe"
!define APP_REG "Software\LumenReader"
!define APP_UNINSTALL "Software\Microsoft\Windows\CurrentVersion\Uninstall\LumenReader"
!define DIST "..\..\lumen-reader-dist"

Name "${APP_NAME} · ${APP_NAME_CN}"
OutFile "..\..\lumen-reader-setup-x64.exe"
InstallDir "$LOCALAPPDATA\Programs\LumenReader"
InstallDirRegKey HKCU "${APP_REG}" "InstallDir"
BrandingText "${APP_NAME} ${APP_VERSION}"

VIProductVersion "0.2.9.0"
VIAddVersionKey "ProductName" "${APP_NAME} · ${APP_NAME_CN}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"
VIAddVersionKey "FileDescription" "Lumen Reader 安装程序"
VIAddVersionKey "CompanyName" "${APP_PUBLISHER}"
VIAddVersionKey "LegalCopyright" "MIT OR Apache-2.0"
VIAddVersionKey "FileVersion" "${APP_VERSION}.0"

!define MUI_ABORTWARNING
!define MUI_ICON "lumen.ico"
!define MUI_UNICON "lumen.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "启动 Lumen Reader 微光阅读"
!define MUI_FINISHPAGE_SHOWREADME "$INSTDIR\使用说明.txt"
!define MUI_FINISHPAGE_SHOWREADME_TEXT "查看使用说明"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"

Section "Lumen Reader 主程序" SEC_MAIN
    SectionIn RO
    SetOutPath "$INSTDIR"
    File /r "${DIST}\*.*"

    CreateDirectory "$SMPROGRAMS\Lumen Reader"
    CreateShortCut "$SMPROGRAMS\Lumen Reader\Lumen Reader.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
    CreateShortCut "$SMPROGRAMS\Lumen Reader\使用说明.lnk" "$INSTDIR\使用说明.txt"
    CreateShortCut "$SMPROGRAMS\Lumen Reader\卸载 Lumen Reader.lnk" "$INSTDIR\Uninstall.exe"
    CreateShortCut "$DESKTOP\Lumen Reader.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0

    WriteUninstaller "$INSTDIR\Uninstall.exe"

    WriteRegStr HKCU "${APP_REG}" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "${APP_REG}" "Version" "${APP_VERSION}"

    WriteRegStr HKCU "${APP_UNINSTALL}" "DisplayName" "${APP_NAME} · ${APP_NAME_CN}"
    WriteRegStr HKCU "${APP_UNINSTALL}" "DisplayVersion" "${APP_VERSION}"
    WriteRegStr HKCU "${APP_UNINSTALL}" "Publisher" "${APP_PUBLISHER}"
    WriteRegStr HKCU "${APP_UNINSTALL}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
    WriteRegStr HKCU "${APP_UNINSTALL}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${APP_UNINSTALL}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKCU "${APP_UNINSTALL}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
    WriteRegDWORD HKCU "${APP_UNINSTALL}" "NoModify" 1
    WriteRegDWORD HKCU "${APP_UNINSTALL}" "NoRepair" 1

    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\lumen-reader.exe" "" "$INSTDIR\${APP_EXE}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\lumen-reader.exe" "Path" "$INSTDIR"
SectionEnd

Section "Uninstall"
    Delete "$DESKTOP\Lumen Reader.lnk"
    Delete "$SMPROGRAMS\Lumen Reader\Lumen Reader.lnk"
    Delete "$SMPROGRAMS\Lumen Reader\使用说明.lnk"
    Delete "$SMPROGRAMS\Lumen Reader\卸载 Lumen Reader.lnk"
    RMDir "$SMPROGRAMS\Lumen Reader"

    DeleteRegKey HKCU "${APP_UNINSTALL}"
    DeleteRegKey HKCU "${APP_REG}"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\lumen-reader.exe"

    RMDir /r "$INSTDIR"

    ; User reading data in %APPDATA%\LumenReader is intentionally kept.
SectionEnd
