!macro NSIS_HOOK_PREINSTALL
  Call RemoveLegacyNovaBaInstallation
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
