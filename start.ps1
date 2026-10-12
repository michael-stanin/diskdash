<#
.SYNOPSIS
    Build (when needed) and start diskdash, then open it in the browser.

.DESCRIPTION
    diskdash is a local, read-only disk space dashboard. It listens on
    127.0.0.1 only. This script never downloads or installs anything: if a
    prerequisite is missing it tells you how to install it and stops.

.PARAMETER Port
    Port to listen on (default 8765).

.PARAMETER Dev
    Development mode: runs the backend with `cargo run --dev` and the Vite dev
    server with hot reload on http://127.0.0.1:5173.

.PARAMETER NoBrowser
    Do not open the browser.
#>
[CmdletBinding()]
param(
    [ValidateRange(1, 65535)][int]$Port = 8765,
    [switch]$Dev,
    [switch]$NoBrowser
)

$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
$Frontend = Join-Path $Root 'frontend'

function Fail([string]$Message) {
    Write-Host ""
    Write-Host "  $Message" -ForegroundColor Red
    Write-Host ""
    exit 1
}

function Step([string]$Message) {
    Write-Host "> $Message" -ForegroundColor Cyan
}

# --- Prerequisites (check only, never install) ------------------------------

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail @"
Rust is not installed (cargo was not found on PATH).
  Install it from https://rustup.rs  (or: winget install Rustlang.Rustup)
  then open a new terminal and run .\start.ps1 again.
"@
}
$rustHost = (& rustc -vV | Select-String '^host:').ToString()
if ($rustHost -notmatch 'msvc') {
    Fail "Rust must target MSVC (found '$rustHost'). Run: rustup default stable-x86_64-pc-windows-msvc"
}

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Fail @"
Node.js is not installed.
  Install Node 20 or newer from https://nodejs.org  (or: winget install OpenJS.NodeJS.LTS)
"@
}
$nodeMajor = [int]((& node --version).TrimStart('v').Split('.')[0])
if ($nodeMajor -lt 20) {
    Fail "Node.js 20 or newer is required (found $(& node --version))."
}

# --- Helpers -----------------------------------------------------------------

function Get-NewestWrite([string[]]$Paths) {
    $newest = [datetime]::MinValue
    foreach ($p in $Paths) {
        if (-not (Test-Path $p)) { continue }
        $item = Get-Item $p
        if ($item.PSIsContainer) {
            $latest = Get-ChildItem $p -Recurse -File | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
            if ($latest -and $latest.LastWriteTimeUtc -gt $newest) { $newest = $latest.LastWriteTimeUtc }
        } elseif ($item.LastWriteTimeUtc -gt $newest) {
            $newest = $item.LastWriteTimeUtc
        }
    }
    $newest
}

function Install-FrontendDeps {
    $modules = Join-Path $Frontend 'node_modules'
    $lock = Join-Path $Frontend 'package-lock.json'
    if (-not (Test-Path $modules) -or (Get-Item $lock).LastWriteTimeUtc -gt (Get-Item $modules).LastWriteTimeUtc) {
        Step 'Installing UI dependencies (npm ci)...'
        Push-Location $Frontend
        try { npm ci --no-audit --no-fund | Out-Host; if ($LASTEXITCODE) { Fail 'npm ci failed.' } }
        finally { Pop-Location }
    }
}

function Wait-Health([string]$Url, [System.Diagnostics.Process]$Process) {
    for ($i = 0; $i -lt 120; $i++) {
        if ($Process -and $Process.HasExited) { return $false }
        try {
            $r = Invoke-RestMethod -Uri "$Url/api/v1/health" -TimeoutSec 2
            if ($r.status -eq 'ok') { return $true }
        } catch { Start-Sleep -Milliseconds 500 }
    }
    $false
}

# --- Dev mode ------------------------------------------------------------------

if ($Dev) {
    Install-FrontendDeps
    Step "Starting backend (dev) on http://127.0.0.1:$Port ..."
    $backend = Start-Process cargo -ArgumentList @('run', '-p', 'diskdash', '--', '--dev', '--port', "$Port") `
        -WorkingDirectory $Root -NoNewWindow -PassThru
    try {
        if (-not (Wait-Health "http://127.0.0.1:$Port" $backend)) { Fail 'The backend did not start. See the output above.' }
        $env:DISKDASH_BACKEND = "http://127.0.0.1:$Port"
        if (-not $NoBrowser) {
            Start-Job { Start-Sleep 3; Start-Process 'http://127.0.0.1:5173/' } | Out-Null
        }
        Step 'Starting the UI dev server on http://127.0.0.1:5173 (Ctrl+C to stop)...'
        Push-Location $Frontend
        try { npm run dev } finally { Pop-Location }
    } finally {
        if (-not $backend.HasExited) { Stop-Process -Id $backend.Id -ErrorAction SilentlyContinue }
    }
    exit 0
}

# --- Normal mode: build when stale, run the release binary --------------------

Install-FrontendDeps

$distIndex = Join-Path $Frontend 'dist\index.html'
$uiSources = @(
    (Join-Path $Frontend 'src'), (Join-Path $Frontend 'public'), (Join-Path $Frontend 'index.html'),
    (Join-Path $Frontend 'package-lock.json'), (Join-Path $Frontend 'vite.config.ts')
)
if (-not (Test-Path $distIndex) -or (Get-NewestWrite $uiSources) -gt (Get-Item $distIndex).LastWriteTimeUtc) {
    Step 'Building the UI...'
    Push-Location $Frontend
    try { npm run build | Out-Host; if ($LASTEXITCODE) { Fail 'The UI build failed.' } }
    finally { Pop-Location }
}

Step 'Building diskdash (cargo build --release; quick when nothing changed)...'
Push-Location $Root
try { cargo build --release -p diskdash; if ($LASTEXITCODE) { Fail 'cargo build failed.' } }
finally { Pop-Location }

$exe = Join-Path $Root 'target\release\diskdash.exe'
$url = "http://127.0.0.1:$Port"
Step "Starting diskdash on $url ..."
$server = Start-Process $exe -ArgumentList @('--port', "$Port") -NoNewWindow -PassThru
try {
    if (-not (Wait-Health $url $server)) {
        Fail "diskdash did not start. If port $Port is busy, run: .\start.ps1 -Port <another port>"
    }
    Write-Host ""
    Write-Host "  diskdash is running at $url   (Ctrl+C to stop)" -ForegroundColor Green
    Write-Host ""
    if (-not $NoBrowser) { Start-Process "$url/" }
    Wait-Process -Id $server.Id
} finally {
    if (-not $server.HasExited) { Stop-Process -Id $server.Id -ErrorAction SilentlyContinue }
}
