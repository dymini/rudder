# Install the native `dymini-panel` CLI on Windows.
#
#   irm https://github.com/vantanminh/rudder/releases/latest/download/install-cli.ps1 | iex
#
# Environment variables:
#   $env:DYMINI_PANEL_GITHUB_REPO   GitHub repo (default: vantanminh/rudder)
#   $env:DYMINI_PANEL_CLI_VERSION   Release tag (default: latest)
#   $env:DYMINI_PANEL_CLI_BIN_DIR   Install directory (default: $HOME\.dymini-panel\bin)

$ErrorActionPreference = "Stop"

$Repo = if ($env:DYMINI_PANEL_GITHUB_REPO) { $env:DYMINI_PANEL_GITHUB_REPO } else { "vantanminh/rudder" }
$Version = if ($env:DYMINI_PANEL_CLI_VERSION) { $env:DYMINI_PANEL_CLI_VERSION } else { "latest" }
$BinDir = if ($env:DYMINI_PANEL_CLI_BIN_DIR) { $env:DYMINI_PANEL_CLI_BIN_DIR } else { Join-Path $HOME ".dymini-panel\bin" }

$Arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "x86_64" }
    "ARM64" { "aarch64" }
    default { throw "Unsupported Windows architecture: $($env:PROCESSOR_ARCHITECTURE)" }
}

$Asset = "dymini-panel-$Arch-pc-windows-msvc.exe"
$BaseUrl = if ($Version -eq "latest") {
    "https://github.com/$Repo/releases/latest/download"
} else {
    $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
    "https://github.com/$Repo/releases/download/$Tag"
}

$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("dymini-panel-cli-" + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null

try {
    $BinTemp = Join-Path $TmpDir $Asset
    $SumsTemp = Join-Path $TmpDir "SHA256SUMS"

    Write-Host "dymini-panel-install: downloading $Asset from $BaseUrl..."
    Invoke-WebRequest -Uri "$BaseUrl/$Asset" -OutFile $BinTemp -UseBasicParsing
    Invoke-WebRequest -Uri "$BaseUrl/SHA256SUMS" -OutFile $SumsTemp -UseBasicParsing

    $Expected = $null
    foreach ($Line in (Get-Content $SumsTemp)) {
        $Parts = $Line -split "\s+", 2
        if ($Parts.Length -eq 2 -and ($Parts[1].TrimStart('*') -eq $Asset)) {
            $Expected = $Parts[0].ToLowerInvariant()
            break
        }
    }
    if (-not $Expected) {
        throw "SHA256SUMS does not list a checksum for $Asset"
    }

    $Actual = (Get-FileHash -Path $BinTemp -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) {
        throw "SHA-256 mismatch for $Asset (expected $Expected, got $Actual)"
    }

    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    $Dest = Join-Path $BinDir "dymini-panel.exe"
    Move-Item -Force -Path $BinTemp -Destination $Dest
    Write-Host "dymini-panel-install: installed dymini-panel to $Dest"

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not ($UserPath -split ";" | Where-Object { $_ -eq $BinDir })) {
        $NewPath = if ($UserPath) { "$UserPath;$BinDir" } else { $BinDir }
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        $env:Path = "$env:Path;$BinDir"
        Write-Host "dymini-panel-install: added $BinDir to your user PATH"
    }

    & $Dest --version
} finally {
    Remove-Item -Recurse -Force -Path $TmpDir -ErrorAction SilentlyContinue
}
