; T8-Lan installer hooks
;
; Tauri 2's NSIS template calls these macros at fixed lifecycle points.
;
; Autostart: T8-Lan draait als Taakplanner-taak bij aanmelden, met de hoogste rechten
; (nodig voor netsh) en zonder UAC-prompt. De taakdefinitie leeft in de app zelf
; (src/autostart.rs: geen accu-beperkingen, geen 72-uurslimiet) - de installer roept
; alleen `T8-Lan.exe --register-autostart` aan zodat de taak ook bestaat als de
; gebruiker de app na installatie niet meteen opent. De app herregistreert de taak
; bovendien bij elke start, zolang autostart in de instellingen aan staat.

!macro NSIS_HOOK_PREINSTALL
  ; Een draaiende instance stoppen, anders kan de exe niet overschreven worden. Dekt ook
  ; de oude binary-naam (t8-lan.exe, t/m v0.2.0): taskkill matcht hoofdletterongevoelig.
  nsExec::ExecToLog 'taskkill.exe /IM "T8-Lan.exe" /F'
  Pop $0
  nsExec::ExecToLog 'taskkill.exe /IM "t8-lan.exe" /F'
  Pop $0
  ; Oude taak (van de vorige installer, met de slechte schtasks-standaardinstellingen)
  ; opruimen; de app maakt zo meteen een nieuwe met de juiste definitie.
  nsExec::ExecToLog 'schtasks.exe /Delete /TN "T8-Lan" /F'
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "T8-Lan: autostart-taak registreren..."
  nsExec::ExecToLog '"$INSTDIR\T8-Lan.exe" --register-autostart'
  Pop $0
  ${If} $0 == 0
    DetailPrint "T8-Lan: autostart-taak aangemaakt."
  ${Else}
    DetailPrint "T8-Lan: WAARSCHUWING - autostart-taak niet aangemaakt (exit $0). De app probeert het opnieuw bij de eerste start."
  ${EndIf}

  ; Firewallregel: de netwerkscan (SADP/ONVIF-multicast) en de DHCP-server (UDP 67)
  ; luisteren op inkomend verkeer. Door hier vooraf een inbound-allow-regel te zetten,
  ; krijgt de monteur nooit de Windows Firewall-prompt te zien. Eerst opruimen om dubbele
  ; regels te vermijden.
  DetailPrint "T8-Lan: firewallregel aanmaken..."
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="T8-Lan"'
  Pop $0
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="T8-Lan" dir=in action=allow program="$INSTDIR\T8-Lan.exe" enable=yes profile=any'
  Pop $0

  ; We willen nooit een bureaublad-snelkoppeling - verwijder hem als die is gemaakt.
  Delete "$DESKTOP\T8-Lan.lnk"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "T8-Lan: autostart-taak verwijderen..."
  nsExec::ExecToLog 'schtasks.exe /End /TN "T8-Lan"'
  Pop $0
  nsExec::ExecToLog 'schtasks.exe /Delete /TN "T8-Lan" /F'
  Pop $0
  ; Firewallregel opruimen.
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="T8-Lan"'
  Pop $0
  ; Stop lopende instance zodat de uninstaller bestanden kan verwijderen (ook oude naam).
  nsExec::ExecToLog 'taskkill.exe /IM "T8-Lan.exe" /F'
  Pop $0
  nsExec::ExecToLog 'taskkill.exe /IM "t8-lan.exe" /F'
  Pop $0
!macroend
