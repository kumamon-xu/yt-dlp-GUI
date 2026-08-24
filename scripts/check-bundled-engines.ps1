param([string]$Root = ".")
$ErrorActionPreference = "Stop"
$yt = Get-ChildItem -Path $Root -Recurse -Filter "yt-dlp.exe" -ErrorAction SilentlyContinue |
    Where-Object { $_.Directory.Name -eq "code" } | Select-Object -First 1
$ff = Get-ChildItem -Path $Root -Recurse -Filter "ffmpeg.exe" -ErrorAction SilentlyContinue |
    Where-Object { $_.Directory.Name -eq "code" } | Select-Object -First 1
if (-not $yt -or -not $ff) { throw "bundled engines missing under $Root/**/code/" }
foreach ($f in @($yt, $ff)) {
    if ($f.Length -lt 1MB) { throw "too small: $($f.FullName)" }
    Write-Host "ok $($f.FullName) $($f.Length)"
}
& $yt.FullName --version
if ($LASTEXITCODE -ne 0) { throw "yt-dlp --version failed ($LASTEXITCODE): $($yt.FullName)" }
& $ff.FullName -version
if ($LASTEXITCODE -ne 0) { throw "ffmpeg -version failed ($LASTEXITCODE): $($ff.FullName)" }
