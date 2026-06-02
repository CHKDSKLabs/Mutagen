<#
.SYNOPSIS
  PowerShell sibling of fetch_harness_binary.sh. Downloads and verifies the
  per-target mutagen-harness archive, extracts the binary into
  plugins/mutagen/bin/. Idempotent.

.PARAMETER Force
  Re-fetch even if a matching binary already exists.

.PARAMETER Quiet
  Suppress informational output.
#>
[CmdletBinding()]
param(
  [switch]$Force,
  [switch]$Quiet
)

$ErrorActionPreference = "Stop"

function Write-Log {
  param([string]$Message)
  if (-not $Quiet) { Write-Host "[fetch-harness] $Message" }
}

function Die {
  param([string]$Message)
  Write-Error "[fetch-harness] $Message"
  exit 1
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$PluginRoot = Resolve-Path (Join-Path $ScriptDir "..")
$Manifest = Join-Path $PluginRoot ".claude-plugin/plugin.json"
$BinDir = Join-Path $PluginRoot "bin"

if (-not (Test-Path $Manifest)) { Die "plugin manifest not found at $Manifest" }

$ManifestJson = Get-Content -Raw -Path $Manifest | ConvertFrom-Json
$Version = $ManifestJson.version
if ([string]::IsNullOrWhiteSpace($Version)) { Die "could not parse version from $Manifest" }

function Get-HostTriple {
  if ($env:MUTAGEN_HARNESS_FORCE_TRIPLE) { return $env:MUTAGEN_HARNESS_FORCE_TRIPLE }

  $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLower()
  if ($IsWindows -or $env:OS -eq "Windows_NT") {
    if ($arch -in @("x64", "x86_64")) { return "x86_64-pc-windows-msvc" }
    Die "unsupported Windows architecture: $arch"
  }
  if ($IsMacOS) {
    switch ($arch) {
      "x64"   { return "x86_64-apple-darwin" }
      "arm64" { return "aarch64-apple-darwin" }
      default { Die "unsupported macOS architecture: $arch" }
    }
  }
  if ($IsLinux) {
    switch ($arch) {
      "x64"   { return "x86_64-unknown-linux-gnu" }
      "arm64" { return "aarch64-unknown-linux-gnu" }
      default { Die "unsupported Linux architecture: $arch" }
    }
  }
  Die "could not detect host OS"
}

$Triple = Get-HostTriple

if ($Triple -like "*windows*") {
  $ArchiveExt = "zip"
  $BinaryName = "mutagen-harness.exe"
} else {
  $ArchiveExt = "tar.gz"
  $BinaryName = "mutagen-harness"
}

$TargetBin = Join-Path $BinDir $BinaryName
$VersionStamp = Join-Path $BinDir ".harness-version"
$StampValue = "$Version-$Triple"

if (-not $Force -and (Test-Path $TargetBin) -and (Test-Path $VersionStamp)) {
  $existing = (Get-Content -Raw -Path $VersionStamp).Trim()
  if ($existing -eq $StampValue) {
    Write-Log "binary already present at $TargetBin (v$Version, $Triple)"
    exit 0
  }
}

$BaseUrl = if ($env:MUTAGEN_HARNESS_RELEASE_BASE_URL) {
  $env:MUTAGEN_HARNESS_RELEASE_BASE_URL
} else {
  "https://github.com/CHKDSKLabs/Mutagen/releases/download"
}

$Asset = "mutagen-harness-v$Version-$Triple.$ArchiveExt"
$AssetUrl = "$BaseUrl/v$Version/$Asset"
$ShaUrl = "$AssetUrl.sha256"

$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("mutagen-harness-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

try {
  $AssetPath = Join-Path $TmpDir $Asset
  $ShaPath = "$AssetPath.sha256"

  Write-Log "fetching $AssetUrl"
  Invoke-WebRequest -Uri $AssetUrl -OutFile $AssetPath -UseBasicParsing
  Invoke-WebRequest -Uri $ShaUrl   -OutFile $ShaPath   -UseBasicParsing

  $expectedSha = ((Get-Content -Raw -Path $ShaPath) -split '\s+')[0].Trim().ToLower()
  if ([string]::IsNullOrWhiteSpace($expectedSha)) { Die "could not parse expected sha256 from $ShaUrl" }

  $actualSha = (Get-FileHash -Path $AssetPath -Algorithm SHA256).Hash.ToLower()
  if ($expectedSha -ne $actualSha) {
    Die "checksum mismatch for $Asset (expected $expectedSha, got $actualSha)"
  }
  Write-Log "checksum verified ($expectedSha)"

  $ExtractDir = Join-Path $TmpDir "extract"
  New-Item -ItemType Directory -Path $ExtractDir -Force | Out-Null

  if ($ArchiveExt -eq "zip") {
    Expand-Archive -Path $AssetPath -DestinationPath $ExtractDir -Force
  } else {
    # Windows 10+ ships bsdtar as tar.exe.
    if (-not (Get-Command tar -ErrorAction SilentlyContinue)) {
      Die "tar is required to extract $Asset and was not found on PATH"
    }
    & tar -xzf $AssetPath -C $ExtractDir
    if ($LASTEXITCODE -ne 0) { Die "tar extraction failed (exit $LASTEXITCODE)" }
  }

  $srcBin = Get-ChildItem -Path $ExtractDir -Recurse -File -Filter $BinaryName | Select-Object -First 1
  if (-not $srcBin) { Die "extracted archive did not contain $BinaryName" }

  if (-not (Test-Path $BinDir)) { New-Item -ItemType Directory -Path $BinDir -Force | Out-Null }
  Copy-Item -Path $srcBin.FullName -Destination $TargetBin -Force
  Set-Content -Path $VersionStamp -Value $StampValue -NoNewline

  Write-Log "installed $TargetBin (v$Version, $Triple)"
}
finally {
  Remove-Item -Path $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
