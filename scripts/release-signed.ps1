<#
    release-signed.ps1 - ondertekende release-build van T8-Lan (Azure Trusted Signing).

    Stappen:
      1. `tauri build`                -> T8-Lan.exe (met bundle-info gepatcht) + gegenereerde NSIS-script
      2. T8-Lan.exe ondertekenen
      3. NSIS opnieuw draaien met de getekende exe en een `!uninstfinalize` die de
         uninstaller ondertekent
      4. de installer ondertekenen en als T8-Lan-v<major.minor>-setup.exe neerzetten
      5. alle handtekeningen controleren

    Waarom niet Tauri's `bundle.windows.signCommand`: die hook faalt in onze opzet
    ("program not found" bij het spawnen), dit script doet exact hetzelfde expliciet.

    Vereist: het Signing-pakket in ..\Signing (lokaal, niet in git) en een geldige
    `az login` op de turn8.io-tenant (zie skill `turn8-signing`).

    Gebruik:  .\scripts\release-signed.ps1 [-SkipBuild]
#>
param([switch] $SkipBuild)
# 'Continue' i.p.v. 'Stop': native tools (node/tauri, signtool, makensis) schrijven
# voortgang naar stderr, wat PowerShell 5.1 anders als terminerende fout ziet. Fouten
# vangen we expliciet op via $LASTEXITCODE.
$ErrorActionPreference = 'Continue'
$root = Split-Path $PSScriptRoot -Parent
$sign = Join-Path $PSScriptRoot 'sign.cmd'
foreach ($dir in 'C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin',
                 'C:\Program Files (x86)\Microsoft SDKs\Azure\CLI2\wbin') {
    if ((Test-Path $dir) -and ($env:PATH -notlike "*$dir*")) { $env:PATH = "$dir;$env:PATH" }
}

Set-Location $root
if (-not $SkipBuild) {
    Write-Host '== 1. tauri build (ongetekend) =='
    npx tauri build
    if ($LASTEXITCODE -ne 0) { throw "tauri build faalde ($LASTEXITCODE)" }
}

$exe = Join-Path $root 'src-tauri\target\release\T8-Lan.exe'
Write-Host '== 2. T8-Lan.exe ondertekenen =='
& $sign $exe
if ($LASTEXITCODE -ne 0) { throw 'ondertekenen van T8-Lan.exe faalde' }

Write-Host '== 3. NSIS opnieuw inpakken met getekende exe + getekende uninstaller =='
$nsiDir = Join-Path $root 'src-tauri\target\release\nsis\x64'
$nsi = Get-Content (Join-Path $nsiDir 'installer.nsi') -Raw
# De template zet `!uninstfinalize` achter een `!if "${UNINSTALLERSIGNCOMMAND}" != ""`;
# aanhalingstekens in die define breken de !if. Daarom vervangen we het hele blok door
# een directe !uninstfinalize (NSIS vult %1 met het pad van de uninstaller).
$uninstCmd = '{0} "%1"' -f $sign   # pad zonder spaties; alleen %1 gequote
$pattern = '!if "\$\{UNINSTALLERSIGNCOMMAND\}" != ""\s*!uninstfinalize ''\$\{UNINSTALLERSIGNCOMMAND\}''\s*!endif'
if ($nsi -notmatch $pattern) { throw 'uninstfinalize-blok niet gevonden in installer.nsi' }
$nsi = [regex]::Replace($nsi, $pattern, "!uninstfinalize '$uninstCmd'")
$signedNsi = Join-Path $nsiDir 'installer-signed.nsi'
Set-Content $signedNsi $nsi -Encoding UTF8
$makensis = Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe'
if (-not (Test-Path $makensis)) { throw "makensis niet gevonden: $makensis (draai eerst tauri build)" }
Push-Location $nsiDir
try {
    & $makensis /V2 $signedNsi
    if ($LASTEXITCODE -ne 0) { throw "makensis faalde ($LASTEXITCODE)" }
} finally { Pop-Location }

Write-Host '== 4. installer ondertekenen =='
$conf = Get-Content (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
$mm = ($conf.version -split '\.')[0..1] -join '.'
$outDir = Join-Path $root 'src-tauri\target\release\bundle\nsis'
New-Item $outDir -ItemType Directory -Force | Out-Null
$setup = Join-Path $outDir "T8-Lan-v$mm-setup.exe"
Copy-Item (Join-Path $nsiDir 'nsis-output.exe') $setup -Force
# De Microsoft-timestampserver geeft af en toe een tijdelijke fout; een paar keer proberen.
$signed = $false
foreach ($attempt in 1..3) {
    & $sign $setup
    if ($LASTEXITCODE -eq 0) { $signed = $true; break }
    Write-Host "ondertekenen van de installer faalde (poging $attempt), opnieuw over 15 s..."
    Start-Sleep -Seconds 15
}
if (-not $signed) { throw 'ondertekenen van de installer faalde' }

Write-Host '== 5. controle =='
foreach ($f in $exe, $setup) {
    $s = Get-AuthenticodeSignature $f
    '{0,-24} {1,-10} {2}' -f (Split-Path $f -Leaf), $s.Status, $s.SignerCertificate.Subject
    if ($s.Status -ne 'Valid') { throw "handtekening ongeldig: $f" }
}
Write-Host "Klaar: $setup"
