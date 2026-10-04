@echo off
rem Code-signing wrapper voor `tauri build --config src-tauri/tauri.sign.conf.json`.
rem Tauri roept dit aan voor T8-Lan.exe, de NSIS-installer en de uninstaller (%1 = pad).
rem Het echte werk doet het Azure Trusted Signing-pakket in ..\Signing (lokaal, niet in
rem git): az-login van de gebruiker + signtool met de Trusted Signing-dlib.
set "SIGNDIR=%~dp0..\Signing"
if not exist "%SIGNDIR%\sign.ps1" (
  echo [sign.cmd] Signing-pakket niet gevonden: %SIGNDIR% 1>&2
  exit /b 1
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%SIGNDIR%\sign.ps1" %*
exit /b %ERRORLEVEL%
