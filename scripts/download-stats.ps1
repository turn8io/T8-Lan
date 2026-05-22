<#
.SYNOPSIS
    Toont het aantal downloads per release-asset van T8-Lan op GitHub, plus het totaal.

.DESCRIPTION
    Roept de GitHub Releases-API (read-only) aan en telt de `download_count` per asset.
    Voor een publieke repo is geen token nodig. Voor een private repo (of om de lage
    rate-limit te omzeilen) kun je een Personal Access Token meegeven via -Token of de
    omgevingsvariabele GITHUB_TOKEN.

.EXAMPLE
    .\scripts\download-stats.ps1

.EXAMPLE
    .\scripts\download-stats.ps1 -Token $env:GITHUB_TOKEN
#>
[CmdletBinding()]
param(
    [string]$Repo = "turn8/T8-LAN",
    [string]$Token = $env:GITHUB_TOKEN
)

$ErrorActionPreference = "Stop"

$headers = @{
    "Accept"               = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent"           = "t8-lan-download-stats"
}
if ($Token) { $headers["Authorization"] = "Bearer $Token" }

$url = "https://api.github.com/repos/$Repo/releases?per_page=100"

try {
    $releases = Invoke-RestMethod -Uri $url -Headers $headers -Method Get
}
catch {
    Write-Error "Ophalen van releases mislukt voor '$Repo': $($_.Exception.Message)"
    exit 1
}

if (-not $releases) {
    Write-Host "Geen releases gevonden voor $Repo."
    exit 0
}

$total = 0
foreach ($rel in $releases) {
    $relCount = 0
    foreach ($asset in $rel.assets) { $relCount += $asset.download_count }
    $total += $relCount

    $tag = if ($rel.tag_name) { $rel.tag_name } else { "(untagged)" }
    $draft = if ($rel.draft) { " [draft]" } else { "" }
    Write-Host ("{0,-16} {1,8} downloads{2}" -f $tag, $relCount, $draft)

    foreach ($asset in $rel.assets) {
        Write-Host ("    {0,-40} {1,8}" -f $asset.name, $asset.download_count)
    }
}

Write-Host ("-" * 40)
Write-Host ("{0,-16} {1,8} downloads (totaal)" -f "TOTAAL", $total)
