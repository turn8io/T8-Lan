<#
    sign.ps1 - code-signing hook voor `tauri build --config src-tauri/tauri.sign.conf.json`.
    Tauri roept dit aan voor T8-Lan.exe, de NSIS-installer en de uninstaller.

    Het echte werk doet het Azure Trusted Signing-pakket in ..\Signing (lokaal, niet in
    git): az-login van de gebruiker + signtool met de Trusted Signing-dlib. Zie de skill
    `turn8-signing`.
#>
param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]] $Files
)
$ErrorActionPreference = 'Stop'

$signing = Join-Path (Split-Path $PSScriptRoot -Parent) 'Signing\sign.ps1'
if (-not (Test-Path $signing)) {
    Write-Error "Signing-pakket niet gevonden: $signing (zet de map Signing naast scripts\)"
    exit 1
}
# De Trusted Signing-dlib gebruikt de az CLI-sessie; az moet daarvoor op PATH staan.
foreach ($dir in 'C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin',
                 'C:\Program Files (x86)\Microsoft SDKs\Azure\CLI2\wbin') {
    if ((Test-Path $dir) -and ($env:PATH -notlike "*$dir*")) { $env:PATH = "$dir;$env:PATH" }
}
& powershell -NoProfile -ExecutionPolicy Bypass -File $signing @Files
exit $LASTEXITCODE
