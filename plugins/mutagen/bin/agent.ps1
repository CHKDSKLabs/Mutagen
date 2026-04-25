#!/usr/bin/env pwsh
# Windows / PowerShell 7+ equivalent of agent.sh.

param(
  [Parameter(Mandatory=$false)][string]$HostKind = $(if ($env:MUTAGEN_HOST) { $env:MUTAGEN_HOST } else { 'codex' }),
  [Parameter(Mandatory=$true, Position=0)][string]$Persona,
  [Parameter(Mandatory=$true, Position=1)][string]$Prompt
)

$ErrorActionPreference = 'Stop'

if (-not $env:MUTAGEN_ROOT) {
  throw "MUTAGEN_ROOT not set — re-run installer or set it manually."
}

$personaFile = Join-Path $env:MUTAGEN_ROOT "agents/$Persona.md"
if (-not (Test-Path $personaFile)) {
  throw "No persona file at $personaFile"
}

$profile = $Persona.ToLower()

# Strip YAML frontmatter
$lines = Get-Content $personaFile
$inFm = $false
$body = foreach ($line in $lines) {
  if ($line -match '^---\s*$') { $inFm = -not $inFm; continue }
  if (-not $inFm) { $line }
}
$personaBody = $body -join "`n"

$framing = @"
# You are $Persona

$personaBody

---

# Current task

$Prompt
"@

if ($env:MUTAGEN_AGENT_LAUNCHER) {
  & $env:MUTAGEN_AGENT_LAUNCHER $HostKind $Persona $profile $framing
  exit $LASTEXITCODE
}

switch ($HostKind) {
  'codex' {
    $codex = if ($env:CODEX_BIN) { $env:CODEX_BIN } else { 'codex' }
    & $codex exec --profile $profile --skip-git-repo-check $framing
    exit $LASTEXITCODE
  }
  'claude' {
    $claude = if ($env:CLAUDE_BIN) { $env:CLAUDE_BIN } else { 'claude' }
    & $claude --print $framing
    exit $LASTEXITCODE
  }
  default {
    throw "Unsupported host '$HostKind'. Set MUTAGEN_AGENT_LAUNCHER to provide a custom launcher."
  }
}
