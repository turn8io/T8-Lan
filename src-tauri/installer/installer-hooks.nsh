; T8-Lan installer hooks
;
; Tauri 2's NSIS template calls these macros at fixed lifecycle points.
; We use them to register/remove a scheduled task that runs T8-Lan at user
; login with the highest available privileges — this gives the app admin
; rights (needed for netsh) without showing a UAC prompt each session.

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "T8-Lan: scheduled task aanmaken voor autostart..."
  nsExec::ExecToLog 'schtasks.exe /Delete /TN "T8-Lan" /F'
  nsExec::ExecToLog 'schtasks.exe /Create /TN "T8-Lan" /TR "\"$INSTDIR\T8-Lan.exe\"" /SC ONLOGON /RL HIGHEST /F'
  Pop $0
  ${If} $0 == 0
    DetailPrint "T8-Lan: scheduled task aangemaakt."
  ${Else}
    DetailPrint "T8-Lan: WAARSCHUWING - scheduled task niet aangemaakt (exit $0)."
  ${EndIf}
  ; Firewallregel: de netwerkscan (Hikvision SADP) opent een UDP-socket en luistert op
  ; inkomend verkeer. Door hier vooraf een inbound-allow-regel te zetten, krijgt de monteur
  ; nooit de Windows Firewall-prompt te zien. Eerst opruimen om dubbele regels te vermijden.
  DetailPrint "T8-Lan: firewallregel aanmaken..."
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="T8-Lan"'
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="T8-Lan" dir=in action=allow program="$INSTDIR\T8-Lan.exe" enable=yes profile=any'

  ; We willen nooit een bureaublad-snelkoppeling — verwijder hem als die is gemaakt.
  Delete "$DESKTOP\T8-Lan.lnk"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "T8-Lan: scheduled task verwijderen..."
  nsExec::ExecToLog 'schtasks.exe /End /TN "T8-Lan"'
  nsExec::ExecToLog 'schtasks.exe /Delete /TN "T8-Lan" /F'
  ; Firewallregel opruimen.
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="T8-Lan"'
  ; Stop lopende instance zodat de uninstaller bestanden kan verwijderen.
  nsExec::ExecToLog 'taskkill.exe /IM "T8-Lan.exe" /F'
!macroend
