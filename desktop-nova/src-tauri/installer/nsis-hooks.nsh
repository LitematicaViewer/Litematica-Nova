!ifndef NOVA_NSIS_HOOKS
!define NOVA_NSIS_HOOKS
!define NOVA_INSTALLER_SOURCES "${__FILEDIR__}"
!define NOVA_INSTALL_DIR_NAME "Litematica-Nova"
!define NOVA_PROGID "LitematicaNova.Projection"
!define NOVA_THUMB_CLSID "{95331DAE-48DC-4BBD-A4E0-E0880D365870}"
!define NOVA_THUMB_HANDLER "{E357FCCD-A995-4576-B01F-234630154E96}"
!define NOVA_SHELL_STATE "Software\Litematica-Nova\ShellIntegration"

; This is the real x86 Unicode NSIS host, not the application's x64 ABI.
!if ${NSIS_PTR_SIZE} != 4
  !error "NovaDui requires the locked x86 NSIS host"
!endif
!if ${NSIS_CHAR_SIZE} != 2
  !error "NovaDui requires the Unicode NSIS host"
!endif
!addplugindir /x86-unicode "${NOVA_INSTALLER_SOURCES}\..\..\..\tools\nsis-duilib\target\x86-unicode"

!macro NOVA_MIGRATE_EXISTING
  NovaDui::Migrate /NOUNLOAD "$INSTDIR"
  Pop $R0
  ${If} $R0 != "ok"
    SetErrorLevel 1603
    Abort "$R0"
  ${EndIf}
!macroend

!macro NOVA_CHECK_RUNNING
  NovaDui::CheckRunning /NOUNLOAD "$INSTDIR"
  Pop $R0
  ${If} $R0 != "ok"
    SetErrorLevel 1618
    Abort "$R0"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  SetRegView 64
  ClearErrors
  WriteRegStr SHCTX "${UNINSTKEY}" "NovaInstallerIdentity" "${BUNDLEID}"
  WriteRegStr SHCTX "${UNINSTKEY}" "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  ReadRegStr $R0 HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" ""
  StrCmp $R0 "${NOVA_THUMB_CLSID}" +2
    WriteRegStr HKLM "${NOVA_SHELL_STATE}" "PreviousThumbnailProvider" "$R0"
  ReadRegStr $R0 HKLM "Software\Classes\.litematic" ""
  ${If} $R0 == ""
  ${OrIf} $R0 == "Litematica projection"
  ${OrIf} $R0 == "${NOVA_PROGID}"
    ${If} $R0 != "${NOVA_PROGID}"
      WriteRegStr HKLM "${NOVA_SHELL_STATE}" "PreviousProgID" "$R0"
    ${EndIf}
    WriteRegStr HKLM "Software\Classes\.litematic" "" "${NOVA_PROGID}"
  ${EndIf}
  WriteRegStr HKLM "Software\Classes\.litematic\OpenWithProgids" "${NOVA_PROGID}" ""
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}" "" "Litematica 投影"
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}\DefaultIcon" "" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0'
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}\shell\open" "MultiSelectModel" "Player"
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}\shell\open\command" "" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\" %*'
  WriteRegStr HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}" "" "Nova Embedded Projection Thumbnail"
  WriteRegStr HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}\InprocServer32" "" "$INSTDIR\litematica_thumbnail.dll"
  WriteRegStr HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}\InprocServer32" "ThreadingModel" "Apartment"
  WriteRegStr HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" "" "${NOVA_THUMB_CLSID}"
  WriteRegStr HKLM "Software\Litematica-Nova\Capabilities" "ApplicationName" "Litematica-Nova"
  WriteRegStr HKLM "Software\Litematica-Nova\Capabilities" "ApplicationDescription" "查看、整理 Minecraft Litematica 投影"
  WriteRegStr HKLM "Software\Litematica-Nova\Capabilities\FileAssociations" ".litematic" "${NOVA_PROGID}"
  WriteRegStr HKLM "Software\RegisteredApplications" "Litematica-Nova" "Software\Litematica-Nova\Capabilities"
  IfErrors 0 +3
    SetErrorLevel 1603
    Abort "Windows 文件入口登记失败"

  SetOutPath "$INSTDIR\licenses\duilib"
  File "${NOVA_INSTALLER_SOURCES}\licenses\LICENSE"
  File "${NOVA_INSTALLER_SOURCES}\licenses\duilib-license.txt"
  File "${NOVA_INSTALLER_SOURCES}\licenses\directui-license.txt"
  File "${NOVA_INSTALLER_SOURCES}\licenses\XUnzip-NOTICE.txt"
  File "${NOVA_INSTALLER_SOURCES}\licenses\WndShadow-NOTICE.txt"
  SetOutPath "$INSTDIR"

  ; Repair this same canonical shortcut even for /UPDATE and repeat installs.
  CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  NovaDui::RepairShell /NOUNLOAD "$INSTDIR"
  Pop $R0
  ${If} $R0 != "ok"
    SetErrorLevel 1603
    Abort "$R0"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  SetRegView 64
  !insertmacro NOVA_CHECK_RUNNING
  ; Unregister only this exact installation; preserve another handler or Editor.
  ReadRegStr $R0 HKLM "Software\Classes\${NOVA_PROGID}\shell\open\command" ""
  ${If} $R0 == '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\" %*'
    DeleteRegValue HKLM "Software\Classes\.litematic\OpenWithProgids" "${NOVA_PROGID}"
    ReadRegStr $R0 HKLM "Software\Classes\.litematic" ""
    ${If} $R0 == "${NOVA_PROGID}"
      ReadRegStr $R1 HKLM "${NOVA_SHELL_STATE}" "PreviousProgID"
      ${If} $R1 == ""
        DeleteRegValue HKLM "Software\Classes\.litematic" ""
      ${Else}
        WriteRegStr HKLM "Software\Classes\.litematic" "" "$R1"
      ${EndIf}
    ${EndIf}
    ReadRegStr $R0 HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" ""
    ${If} $R0 == "${NOVA_THUMB_CLSID}"
      ReadRegStr $R1 HKLM "${NOVA_SHELL_STATE}" "PreviousThumbnailProvider"
      ${If} $R1 == ""
        DeleteRegKey HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}"
      ${Else}
        WriteRegStr HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" "" "$R1"
      ${EndIf}
    ${EndIf}
    DeleteRegKey HKLM "Software\Classes\${NOVA_PROGID}"
    DeleteRegKey HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}"
    DeleteRegValue HKLM "Software\RegisteredApplications" "Litematica-Nova"
    DeleteRegKey HKLM "Software\Litematica-Nova\Capabilities"
    DeleteRegKey HKLM "${NOVA_SHELL_STATE}"
  ${EndIf}
  ReadRegStr $R0 HKLM "Software\Classes\Applications\Litematica-Nova.exe\shell\open\command" ""
  ${If} $R0 == '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\" %*'
    DeleteRegKey HKLM "Software\Classes\Applications\Litematica-Nova.exe"
  ${EndIf}
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Installer-owned files only; unknown files and user data survive.
  Delete "$INSTDIR\licenses\duilib\LICENSE"
  Delete "$INSTDIR\licenses\duilib\duilib-license.txt"
  Delete "$INSTDIR\licenses\duilib\directui-license.txt"
  Delete "$INSTDIR\licenses\duilib\XUnzip-NOTICE.txt"
  Delete "$INSTDIR\licenses\duilib\WndShadow-NOTICE.txt"
  RMDir "$INSTDIR\licenses\duilib"
  RMDir "$INSTDIR\licenses"
  RMDir "$INSTDIR"
!macroend
!endif
