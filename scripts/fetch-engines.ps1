# Download yt-dlp.exe + ffmpeg.exe into code/ (gitignored).
# Used by GitHub Actions and local setup:  powershell -File scripts/fetch-engines.ps1
param(
    [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Code = Join-Path $Root "code"
New-Item -ItemType Directory -Force -Path $Code | Out-Null

function Test-Engine([string]$Name) {
    $p = Join-Path $Code $Name
    return (Test-Path $p) -and ((Get-Item $p).Length -gt 1MB)
}

function Invoke-Download([string]$Url, [string]$OutFile) {
    Write-Host "Downloading $Url"
    & curl.exe -L --fail --retry 3 --retry-delay 2 --connect-timeout 30 -o $OutFile $Url
    if ($LASTEXITCODE -ne 0) {
        throw "download failed ($LASTEXITCODE): $Url"
    }
}

function Get-GithubJson([string]$Url) {
    $headers = @{
        "User-Agent" = "yt-dlp-gui-ci"
        "Accept"     = "application/vnd.github+json"
    }
    if ($env:GITHUB_TOKEN) {
        $headers["Authorization"] = "Bearer $($env:GITHUB_TOKEN)"
    }
    Invoke-RestMethod -Uri $Url -Headers $headers
}

if (-not $Force -and (Test-Engine "yt-dlp.exe") -and (Test-Engine "ffmpeg.exe")) {
    Write-Host "Engines already present in code/ (pass -Force to re-download)."
    exit 0
}

# --- yt-dlp (Windows x64) ---
$Ytdlp = Join-Path $Code "yt-dlp.exe"
if ($Force -or -not (Test-Engine "yt-dlp.exe")) {
    Invoke-Download "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" $Ytdlp
}

# --- ffmpeg essentials (GyanD; fallback BtbN) ---
if ($Force -or -not (Test-Engine "ffmpeg.exe")) {
    $Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ffmpeg-" + [guid]::NewGuid().ToString("n"))
    New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
    try {
        $Zip = Join-Path $Tmp "ffmpeg.zip"
        try {
            $rel = Get-GithubJson "https://api.github.com/repos/GyanD/codexffmpeg/releases/latest"
            $asset = $rel.assets | Where-Object { $_.name -like "*essentials_build.zip" } | Select-Object -First 1
            if (-not $asset) { throw "no essentials_build.zip on GyanD latest" }
            Invoke-Download $asset.browser_download_url $Zip
        }
        catch {
            Write-Warning "GyanD ffmpeg failed ($($_.Exception.Message)); falling back to BtbN."
            Invoke-Download "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip" $Zip
        }

        $Out = Join-Path $Tmp "out"
        Expand-Archive -Path $Zip -DestinationPath $Out -Force
        $exe = Get-ChildItem -Path $Out -Recurse -Filter ffmpeg.exe | Select-Object -First 1
        if (-not $exe) { throw "ffmpeg.exe missing from archive" }
        Copy-Item $exe.FullName (Join-Path $Code "ffmpeg.exe") -Force
    }
    finally {
        Remove-Item $Tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Get-ChildItem $Code -Filter *.exe | ForEach-Object {
    Write-Host ("  {0,-12} {1:N1} MB" -f $_.Name, ($_.Length / 1MB))
}

if (-not (Test-Engine "yt-dlp.exe") -or -not (Test-Engine "ffmpeg.exe")) {
    throw "engines were not downloaded correctly"
}
Write-Host "Engines ready in $Code"
