# Build Aurivon Windows release artifacts:
#   dist/aurivon-<ver>-x86_64-windows.zip              (updater / portable)
#   dist/aurivon-<ver>-x86_64-windows-setup.exe        (Inno Setup installer)
#   matching .sha256 sidecars
#
# Bundles official mpv Windows full build next to aurivon.exe under mpv/.
param(
    [switch]$SkipBuild,
    [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
$Repo = Split-Path -Parent $PSScriptRoot
Set-Location $Repo

function Get-WorkspaceVersion {
    $inPkg = $false
    foreach ($line in Get-Content (Join-Path $Repo "Cargo.toml")) {
        if ($line -match '^\[workspace\.package\]') { $inPkg = $true; continue }
        if ($inPkg -and $line -match '^\[') { break }
        if ($inPkg -and $line -match '^version\s*=\s*"([^"]+)"') { return $Matches[1] }
    }
    throw "Could not read workspace.package version from Cargo.toml"
}

$Version = Get-WorkspaceVersion
$Name = "aurivon-$Version"
$Stage = Join-Path $Repo "dist\$Name"
$Dist = Join-Path $Repo "dist"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

if (-not $SkipBuild) {
    Write-Host "Building aurivon $Version (release)..."
    cargo build --release --bin aurivon
}

$Exe = Join-Path $Repo "target\release\aurivon.exe"
if (-not (Test-Path $Exe)) { throw "Missing $Exe" }
$reported = & $Exe --version
if ("$reported" -notmatch [regex]::Escape($Version)) {
    throw "release binary reported '$reported', expected version $Version"
}

# Fresh stage
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item $Exe (Join-Path $Stage "aurivon.exe")
Copy-Item (Join-Path $Repo "README.md") $Stage -ErrorAction SilentlyContinue
Copy-Item (Join-Path $Repo "LICENSE") $Stage -ErrorAction SilentlyContinue
if (Test-Path (Join-Path $Repo "CHANGELOG.md")) {
    Copy-Item (Join-Path $Repo "CHANGELOG.md") $Stage
}
Set-Content -Path (Join-Path $Stage "VERSION") -Value $Version -NoNewline
@"
Aurivon for Windows
===================

This build includes a bundled mpv player (mpv\mpv.exe).

Installed via Setup:
  Start Menu → Aurivon
  Files under %LOCALAPPDATA%\aurivon\app\versions\<version>\
  Active version: %LOCALAPPDATA%\aurivon\app\current\

Portable / updater zip:
  Unzip and run aurivon.exe, or let the in-app updater install the zip
  into the versions folder after Setup has been run once.
"@ | Set-Content -Path (Join-Path $Stage "INSTALL.txt")

# Fetch official mpv Windows full build (zip) from the latest GitHub release
Write-Host "Fetching bundled mpv (mpv-player/mpv latest full x86_64)..."
$mpvApi = Invoke-RestMethod -Headers @{ "User-Agent" = "Aurivon-packager" } `
    -Uri "https://api.github.com/repos/mpv-player/mpv/releases/latest"
# Names look like mpv-v0.42.0-x86_64-w64-mingw32-full.zip (version sits between mpv- and arch).
$mpvAsset = $mpvApi.assets | Where-Object {
    $_.name -match '^mpv-.*-x86_64-w64-mingw32-full\.zip$'
} | Select-Object -First 1
if (-not $mpvAsset) {
    # Fallback: MSVC player build (not PDB symbols, not libmpv)
    $mpvAsset = $mpvApi.assets | Where-Object {
        $_.name -match '^mpv-.*-x86_64-pc-windows-msvc\.zip$'
    } | Select-Object -First 1
}
if (-not $mpvAsset) {
    $mpvAsset = $mpvApi.assets | Where-Object {
        $_.name -match 'x86_64' -and $_.name -match '\.zip$' -and $_.name -notmatch 'libmpv|debug|dev|pdb|\.asc$'
    } | Select-Object -First 1
}
if (-not $mpvAsset) { throw "Could not find an mpv Windows x86_64 zip on $($mpvApi.tag_name)" }

$mpvZip = Join-Path $Dist "mpv-bundle.zip"
Write-Host "Downloading $($mpvAsset.name)..."
Invoke-WebRequest -Uri $mpvAsset.browser_download_url -OutFile $mpvZip -UseBasicParsing
$mpvExtract = Join-Path $Dist "mpv-extract"
if (Test-Path $mpvExtract) { Remove-Item -Recurse -Force $mpvExtract }
Expand-Archive -Path $mpvZip -DestinationPath $mpvExtract -Force

# Normalize into stage\mpv\mpv.exe (+ deps)
$mpvDir = Join-Path $Stage "mpv"
New-Item -ItemType Directory -Force -Path $mpvDir | Out-Null
$found = Get-ChildItem -Path $mpvExtract -Recurse -Filter "mpv.exe" | Select-Object -First 1
if (-not $found) { throw "mpv.exe not found inside $($mpvAsset.name)" }
Copy-Item -Path (Join-Path $found.DirectoryName "*") -Destination $mpvDir -Recurse -Force
Write-Host "Bundled mpv from $($found.DirectoryName)"

# Updater / portable zip
$ZipName = "$Name-x86_64-windows.zip"
$ZipPath = Join-Path $Dist $ZipName
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
# Compress-Archive wants the folder contents under aurivon-VER/
$ZipRoot = Join-Path $Dist "zip-root"
if (Test-Path $ZipRoot) { Remove-Item -Recurse -Force $ZipRoot }
New-Item -ItemType Directory -Force -Path (Join-Path $ZipRoot $Name) | Out-Null
Copy-Item -Path (Join-Path $Stage "*") -Destination (Join-Path $ZipRoot $Name) -Recurse -Force
Compress-Archive -Path (Join-Path $ZipRoot $Name) -DestinationPath $ZipPath -Force
Remove-Item -Recurse -Force $ZipRoot

function Write-Sha256([string]$Path) {
    $hash = (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
    $name = Split-Path -Leaf $Path
    Set-Content -Path "$Path.sha256" -Value "$hash  $name" -NoNewline
}
Write-Sha256 $ZipPath
Write-Host "Wrote $ZipPath"

if (-not $SkipInstaller) {
    $Iscc = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles}\Inno Setup 6\ISCC.exe",
        "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1

    if (-not $Iscc) {
        Write-Warning "Inno Setup 6 (ISCC.exe) not found — skipping setup.exe (zip still built)"
    } else {
        $Iss = Join-Path $Repo "packaging\windows\aurivon.iss"
        & $Iscc "/DMyAppVersion=$Version" "/DSourceDir=$Stage" $Iss
        $Setup = Join-Path $Dist "$Name-x86_64-windows-setup.exe"
        if (-not (Test-Path $Setup)) { throw "Expected installer at $Setup" }
        Write-Sha256 $Setup
        Write-Host "Wrote $Setup"
    }
}

Write-Host $ZipPath
