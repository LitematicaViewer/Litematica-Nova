!macro NSIS_HOOK_PREINSTALL
  Call RemoveLegacyNovaBaInstallation
!macroend

!define NOVA_PROGID "LitematicaNova.Projection"
!define NOVA_THUMB_CLSID "{95331DAE-48DC-4BBD-A4E0-E0880D365870}"
!define NOVA_THUMB_HANDLER "{E357FCCD-A995-4576-B01F-234630154E96}"
!define NOVA_SHELL_STATE "Software\Litematica-Nova\ShellIntegration"

!macro NSIS_HOOK_POSTINSTALL
  SetRegView 64
  ReadRegStr $R0 HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" ""
  StrCmp $R0 "${NOVA_THUMB_CLSID}" +2
    WriteRegStr HKLM "${NOVA_SHELL_STATE}" "PreviousThumbnailProvider" "$R0"
  ReadRegStr $R0 HKLM "Software\Classes\.litematic" ""
  StrCmp $R0 "" 0 +3
    WriteRegStr HKLM "Software\Classes\.litematic" "" "${NOVA_PROGID}"
    WriteRegDWORD HKLM "${NOVA_SHELL_STATE}" "SetDefault" 1
  WriteRegStr HKLM "Software\Classes\.litematic\OpenWithProgids" "${NOVA_PROGID}" ""
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}" "" "Litematica 投影"
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}\DefaultIcon" "" '$"$INSTDIR\Litematica-Nova.exe$",0'
  WriteRegStr HKLM "Software\Classes\${NOVA_PROGID}\shell\open\command" "" '$"$INSTDIR\Litematica-Nova.exe$" $"%1$"'
  WriteRegStr HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}" "" "Nova Cached Projection Thumbnail"
  WriteRegStr HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}\InprocServer32" "" "$INSTDIR\litematica_thumbnail.dll"
  WriteRegStr HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}\InprocServer32" "ThreadingModel" "Apartment"
  WriteRegStr HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" "" "${NOVA_THUMB_CLSID}"
  WriteRegStr HKLM "Software\Litematica-Nova\Capabilities" "ApplicationName" "Litematica-Nova"
  WriteRegStr HKLM "Software\Litematica-Nova\Capabilities" "ApplicationDescription" "查看、整理 Minecraft Litematica 投影"
  WriteRegStr HKLM "Software\Litematica-Nova\Capabilities\FileAssociations" ".litematic" "${NOVA_PROGID}"
  WriteRegStr HKLM "Software\RegisteredApplications" "Litematica-Nova" "Software\Litematica-Nova\Capabilities"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  SetRegView 64
  ; Only unregister this installation; preserve user choice and another installation.
  ReadRegStr $R0 HKLM "Software\Classes\${NOVA_PROGID}\shell\open\command" ""
  StrCmp $R0 '$"$INSTDIR\Litematica-Nova.exe$" $"%1$"' 0 nova_shell_unregister_done
  DeleteRegValue HKLM "Software\Classes\.litematic\OpenWithProgids" "${NOVA_PROGID}"
  ReadRegStr $R0 HKLM "Software\Classes\.litematic" ""
  StrCmp $R0 "${NOVA_PROGID}" 0 +2
    DeleteRegValue HKLM "Software\Classes\.litematic" ""
  ReadRegStr $R0 HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" ""
  StrCmp $R0 "${NOVA_THUMB_CLSID}" 0 nova_shell_thumbnail_done
  ReadRegStr $R1 HKLM "${NOVA_SHELL_STATE}" "PreviousThumbnailProvider"
  StrCmp $R1 "" 0 +3
    DeleteRegKey HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}"
    Goto nova_shell_thumbnail_done
  WriteRegStr HKLM "Software\Classes\.litematic\shellex\${NOVA_THUMB_HANDLER}" "" "$R1"
  nova_shell_thumbnail_done:
  DeleteRegKey HKLM "Software\Classes\${NOVA_PROGID}"
  DeleteRegKey HKLM "Software\Classes\CLSID\${NOVA_THUMB_CLSID}"
  DeleteRegValue HKLM "Software\RegisteredApplications" "Litematica-Nova"
  DeleteRegKey HKLM "Software\Litematica-Nova\Capabilities"
  DeleteRegKey HKLM "${NOVA_SHELL_STATE}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  nova_shell_unregister_done:
!macroend

Function RemoveLegacyNovaBaInstallation
  ; An older MSI used a different product name, so Tauri's normal WiX-to-NSIS
  ; migration cannot discover it by the current DisplayName. Uninstall only
  ; that exact legacy product when present.
  SetRegView 64
  StrCpy $R0 0

  legacy_uninstall_loop:
    EnumRegKey $R1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $R0
    StrCmp $R1 "" legacy_shortcut_cleanup
    IntOp $R0 $R0 + 1
    ReadRegStr $R2 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$R1" "DisplayName"
    StrCmp $R2 "Litematica Nova BA" 0 legacy_uninstall_loop
    ReadRegStr $R3 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$R1" "UninstallString"
    StrCmp $R3 "" legacy_shortcut_cleanup
    ExecWait '$R3 /qn /norestart' $R4

  legacy_shortcut_cleanup:
    ; Remove only known installer-owned links. RMDir intentionally has no /r,
    ; so an old directory containing unknown/user files is preserved.
    Delete "$DESKTOP\Litematica Nova BA.lnk"
    Delete "$SMPROGRAMS\Litematica Nova BA\Litematica Nova BA.lnk"
    RMDir "$SMPROGRAMS\Litematica Nova BA"
    Delete "$PROGRAMFILES64\Litematica Nova BA\Uninstall Litematica Nova BA.lnk"
    RMDir "$PROGRAMFILES64\Litematica Nova BA"

    ; A prior WiX install can leave these installer-owned links behind when it
    ; was itself replaced through a major-upgrade chain. NSIS uses uninstall.exe
    ; and a direct Start Menu shortcut, so these exact WiX links are obsolete.
    Delete "$PROGRAMFILES64\Litematica-Nova\Uninstall Litematica-Nova.lnk"
    Delete "$SMPROGRAMS\Litematica-Nova\Litematica-Nova.lnk"
    RMDir "$SMPROGRAMS\Litematica-Nova"
FunctionEnd
