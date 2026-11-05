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

; Shell links use a shell path, not a verbatim filesystem path. Keep the
; original $INSTDIR intact for file I/O, upgrade identity and the /D= handoff.
Var NovaShortcutDir
!macro NOVA_SHORTCUT_DIRECTORY
  NovaDui::DirectoryDisplayPath /NOUNLOAD "$INSTDIR"
  Pop $NovaShortcutDir
!macroend
!macro NOVA_CREATE_SHORTCUT link
  ; NSIS copies $OUTDIR into the link's working directory. Set just the
  ; variable for this instruction, without changing the process directory.
  Push $OUTDIR
  StrCpy $OUTDIR $NovaShortcutDir
  ClearErrors
  CreateShortcut "${link}" "$NovaShortcutDir\${MAINBINARYNAME}.exe"
  Pop $OUTDIR
!macroend

!macro NOVA_MIGRATE_EXISTING
  NovaDui::SetLanguage /NOUNLOAD "$LANGUAGE"
  NovaDui::Migrate /NOUNLOAD "$INSTDIR"
  Pop $R0
  ${If} $R0 != "ok"
    SetErrorLevel 1603
    Abort "$R0"
  ${EndIf}
!macroend

!macro NOVA_CHECK_RUNNING
  NovaDui::SetLanguage /NOUNLOAD "$LANGUAGE"
  NovaDui::CheckRunning /NOUNLOAD "$INSTDIR"
  Pop $R0
  ${If} $R0 != "ok"
    SetErrorLevel 1618
    Abort "$R0"
  ${EndIf}
!macroend

Var NovaPrunePreviousLayout
Var NovaSameInstallDir
Var NovaPrunedFileCount
Var NovaBusyFileCount
!macro NSIS_HOOK_PREINSTALL
  StrCpy $NovaPrunePreviousLayout 0
  StrCpy $NovaSameInstallDir 0
  SetRegView 64
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" "InstallLocation"
  ReadRegStr $R1 SHCTX "${UNINSTKEY}" "NovaInstallerIdentity"
  ${If} $R0 != ""
  ${AndIf} $R1 == "${BUNDLEID}"
    NovaDui::DirectoryDisplayPath /NOUNLOAD "$R0"
    Pop $R0
    NovaDui::DirectoryDisplayPath /NOUNLOAD "$INSTDIR"
    Pop $R1
    ${If} $R0 == $R1
      StrCpy $NovaSameInstallDir 1
      IfFileExists "$INSTDIR\pack-in\game-data\default\derived\version-adapters\*.zip" 0 +2
        StrCpy $NovaPrunePreviousLayout 1
      IfFileExists "$INSTDIR\pack-in\game-data\default\assets\*.*" 0 +2
        StrCpy $NovaPrunePreviousLayout 1
    ${EndIf}
  ${EndIf}
  ClearErrors
!macroend

; Compiled from exact retired package receipts, never from the installation
; being upgraded. A resource reintroduced by this Setup always wins.
!macro NOVA_PRUNE_FILE relative bytes digest
  !ifndef "NOVA_CURRENT_RESOURCE_${relative}"
    NovaDui::PruneRetiredFile /NOUNLOAD "$INSTDIR" "${relative}" "${bytes}" "${digest}"
    Pop $R0
    ${If} $R0 == "removed"
      IntOp $NovaPrunedFileCount $NovaPrunedFileCount + 1
    ${ElseIf} $R0 == "busy"
      IntOp $NovaBusyFileCount $NovaBusyFileCount + 1
      DetailPrint "Previous Nova package file retained (in use or inaccessible): ${relative}"
    ${EndIf}
  !endif
!macroend

!macro NOVA_PRUNE_RETIRED_INSTALL_FILES
  ; Previous identity and directory were checked before writing the new registration.
  ${If} $NovaSameInstallDir == 1
    StrCpy $NovaPrunedFileCount 0
    StrCpy $NovaBusyFileCount 0
    !include "${NOVA_INSTALLER_SOURCES}\retired-files.nsh"
    DetailPrint "Previous Nova package files removed: $NovaPrunedFileCount; inaccessible: $NovaBusyFileCount."
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  SetRegView 64
  ; Missing previous associations are normal on a first install. ReadRegStr
  ; sets NSIS's sticky error flag; keep optional reads outside the write group
  ; so they cannot turn successful registration into an installation failure.
  ReadRegStr $R0 HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" ""
  ReadRegStr $R1 HKLM "Software\Classes\.litematic" ""
  ClearErrors
  WriteRegStr SHCTX "${UNINSTKEY}" "NovaInstallerIdentity" "${BUNDLEID}"
  WriteRegStr SHCTX "${UNINSTKEY}" "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  StrCmp $R0 "${NOVA_THUMB_CLSID}" +2
    WriteRegStr HKLM "${NOVA_SHELL_STATE}" "PreviousThumbnailProvider" "$R0"
  ${If} $R1 == ""
  ${OrIf} $R1 == "Litematica projection"
  ${OrIf} $R1 == "${NOVA_PROGID}"
    ${If} $R1 != "${NOVA_PROGID}"
      WriteRegStr HKLM "${NOVA_SHELL_STATE}" "PreviousProgID" "$R1"
    ${EndIf}
    WriteRegStr HKLM "Software\Classes\.litematic" "" "${NOVA_PROGID}"
  ${EndIf}
  WriteRegStr HKLM "Software\Classes\.litematic\OpenWithProgids" "${NOVA_PROGID}" ""
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}" "" "$(novaSchematicType)"
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}\DefaultIcon" "" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0'
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}\shell\open" "MultiSelectModel" "Player"
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}\shell\open\command" "" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\" %*'
  WriteRegStr HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}" "" "Nova Embedded Projection Thumbnail"
  WriteRegStr HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}\InprocServer32" "" "$INSTDIR\litematica_thumbnail.dll"
  WriteRegStr HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}\InprocServer32" "ThreadingModel" "Apartment"
  WriteRegStr HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" "" "${NOVA_THUMB_CLSID}"
  WriteRegStr HKLM "Software\Litematica-Nova\Capabilities" "ApplicationName" "Litematica-Nova"
  WriteRegStr HKLM "Software\Litematica-Nova\Capabilities" "ApplicationDescription" "$(novaApplicationDescription)"
  WriteRegStr HKLM "Software\Litematica-Nova\Capabilities\FileAssociations" ".litematic" "${NOVA_PROGID}"
  WriteRegStr HKLM "Software\RegisteredApplications" "Litematica-Nova" "Software\Litematica-Nova\Capabilities"
  IfErrors 0 +3
    SetErrorLevel 1603
    Abort "$(novaRegistrationFailed)"

  SetOutPath "$INSTDIR\licenses\duilib"
  File "${NOVA_INSTALLER_SOURCES}\licenses\LICENSE"
  File "${NOVA_INSTALLER_SOURCES}\licenses\duilib-license.txt"
  File "${NOVA_INSTALLER_SOURCES}\licenses\directui-license.txt"
  File "${NOVA_INSTALLER_SOURCES}\licenses\XUnzip-NOTICE.txt"
  File "${NOVA_INSTALLER_SOURCES}\licenses\WndShadow-NOTICE.txt"
  SetOutPath "$INSTDIR"

  ; The start menu function owns the current shortcut, including on /UPDATE.
  ; RepairShell distinguishes required registration from optional old links.
  NovaDui::SetLanguage /NOUNLOAD "$LANGUAGE"
  NovaDui::RepairShell /NOUNLOAD "$INSTDIR"
  Pop $R0
  StrCpy $R1 $R0 9
  ${If} $R1 == "warning: "
    StrCpy $R1 $R0 "" 9
    DetailPrint "$R1"
  ${ElseIf} $R0 != "ok"
    SetErrorLevel 1603
    Abort "$R0"
  ${EndIf}
  !ifdef NOVA_PRUNE_ADAPTERS
    !ifndef NOVA_PRUNE_CORE
      !error "Compact game data requires the packaged core helper"
    !endif
    !ifndef NOVA_PRUNE_RESOURCES
      !error "Compact game data requires its resource index"
    !endif
    ${If} $NovaPrunePreviousLayout == 1
      InitPluginsDir
      SetOutPath "$PLUGINSDIR"
      File /oname=nova-prune.exe "${NOVA_PRUNE_CORE}"
      File /oname=nova-resources.json "${NOVA_PRUNE_RESOURCES}"
      File /oname=nova-archives.json "${NOVA_PRUNE_ADAPTERS}"
      nsExec::ExecToLog '"$PLUGINSDIR\nova-prune.exe" prune-replaced-install-resources "$INSTDIR" "$PLUGINSDIR\nova-resources.json" "$PLUGINSDIR\nova-archives.json"'
      Pop $R0
      SetOutPath "$INSTDIR"
      ${If} $R0 != 0
        DetailPrint "Previous resource cleanup skipped ($R0); previous files retained."
      ${EndIf}
    ${EndIf}
  !endif
  !insertmacro NOVA_PRUNE_RETIRED_INSTALL_FILES
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
