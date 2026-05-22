# Hernoemt de zojuist gebouwde NSIS-installer naar een vaste naam.
# Tauri produceert "T8-Lan_<versie>_x64-setup.exe"; wij willen alleen
# "T8-Lan-v0.1-setup.exe" overhouden (geen dubbele exe). Draait ná
# `tauri build`, dus verplaatsen is veilig.
$ErrorActionPreference = "Stop"
$dir = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle\nsis"
$target = "T8-Lan-v0.1-setup.exe"

if (-not (Test-Path $dir)) {
  Write-Output "Geen NSIS-bundle map gevonden: $dir"
  exit 0
}

$src = Get-ChildItem -Path $dir -Filter "*-setup.exe" |
  Where-Object { $_.Name -ne $target } |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1

if ($null -eq $src) {
  Write-Output "Geen installer gevonden in $dir"
  exit 0
}

$dst = Join-Path $dir $target
Move-Item $src.FullName $dst -Force
Write-Output "Installer klaar: $dst"
