# Download yt-dlp.exe + ffmpeg.exe into code/.
#   -Lock    engines.lock.json (Stable, default in CI tags)
#   -Latest  ignore lock (Nightly)
param(
    [switch]$Force,
    [switch]$Lock,
    [switch]$Latest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Code = Join-Path $Root "code"
$LockFile = Join-Path $Root "engines.lock.json"
New-Item -ItemType Directory -Force -Path $Code | Out-Null

function Invoke-Download([string]$Url, [string]$OutFile) {
    Write-Host "Downloading $Url"
    & curl.exe -L --fail --retry 3 --retry-delay 2 --connect-timeout 30 -o $OutFile $Url
    if ($LASTEXITCODE -ne 0) { throw "download failed ($LASTEXITCODE): $Url" }
}

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLowerInvariant()
}

function Assert-LicenseFile($ToolNode, [string]$ToolName) {
    if (-not $ToolNode.licenseFile -or -not $ToolNode.licenseSha256) {
        throw "missing license metadata for $ToolName"
    }
    $path = Join-Path $Root $ToolNode.licenseFile
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "missing license file: $path"
    }
    $got = Get-Sha256 $path
    if ($got -ne $ToolNode.licenseSha256.ToLowerInvariant()) {
        throw "$ToolName license hash mismatch: expected $($ToolNode.licenseSha256) got $got"
    }
}

$useLock = -not $Latest
if ($Lock) { $useLock = $true }

$lock = $null
if ($useLock) {
    if (-not (Test-Path -LiteralPath $LockFile -PathType Leaf)) {
        throw "missing lock file: $LockFile"
    }
    $lock = Get-Content $LockFile -Raw | ConvertFrom-Json
    Assert-LicenseFile $lock.'yt-dlp' 'yt-dlp'
    Assert-LicenseFile $lock.ffmpeg 'ffmpeg'
}

$Ytdlp = Join-Path $Code "yt-dlp.exe"
$Ffmpeg = Join-Path $Code "ffmpeg.exe"

if (-not $Force -and (Test-Path $Ytdlp) -and (Test-Path $Ffmpeg)) {
    if ((Get-Item $Ytdlp).Length -gt 1MB -and (Get-Item $Ffmpeg).Length -gt 1MB) {
        $existingValid = $true
        if ($useLock) {
            $existingValid = (Get-Sha256 $Ytdlp) -eq $lock.'yt-dlp'.'windows-x64'.sha256.ToLowerInvariant() -and
                (Get-Sha256 $Ffmpeg) -eq $lock.ffmpeg.'windows-x64'.sha256.ToLowerInvariant()
            if (-not $existingValid) { Write-Host "existing engines do not match lock; re-downloading" }
        }
        if ($existingValid) {
            Write-Host "engines already in $Code (pass -Force to re-download)"
            & $Ytdlp --version
            & $Ffmpeg -version | Select-Object -First 1
            exit 0
        }
    }
}

if ($useLock) {
    $yt = $lock.'yt-dlp'.'windows-x64'
    Invoke-Download $yt.url $Ytdlp
    $got = Get-Sha256 $Ytdlp
    if ($got -ne $yt.sha256.ToLowerInvariant()) {
        throw "yt-dlp hash mismatch: expected $($yt.sha256) got $got"
    }
    $ff = $lock.ffmpeg.'windows-x64'
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("ff-" + [guid]::NewGuid().ToString("n"))
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        $zip = Join-Path $tmp "ffmpeg.zip"
        Invoke-Download $ff.url $zip
        Expand-Archive $zip (Join-Path $tmp "out") -Force
        $exe = Get-ChildItem -Path (Join-Path $tmp "out") -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
        if (-not $exe) { throw "ffmpeg.exe missing from archive" }
        Copy-Item $exe.FullName $Ffmpeg -Force
    } finally {
        Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
    $got = Get-Sha256 $Ffmpeg
    if ($got -ne $ff.sha256.ToLowerInvariant()) {
        throw "ffmpeg hash mismatch: expected $($ff.sha256) got $got"
    }
} else {
    Invoke-Download "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" $Ytdlp
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("ff-" + [guid]::NewGuid().ToString("n"))
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        $zip = Join-Path $tmp "ffmpeg.zip"
        Invoke-Download "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip" $zip
        Expand-Archive $zip (Join-Path $tmp "out") -Force
        $exe = Get-ChildItem -Path (Join-Path $tmp "out") -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
        Copy-Item $exe.FullName $Ffmpeg -Force
    } finally {
        Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Get-ChildItem $Code -Filter *.exe | ForEach-Object {
    Write-Host ("  {0,-12} {1:N1} MB" -f $_.Name, ($_.Length / 1MB))
}
& $Ytdlp --version | Select-Object -First 1 | Write-Host
& $Ffmpeg -version | Select-Object -First 1 | Write-Host
Write-Host "Engines ready in $Code"
