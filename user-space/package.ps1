#Requires -Version 5.1
# Build release navigator.exe, stage user-space/, create navigator-vVERSION-windows-x86_64.zip
# Run from repo root:  powershell -ExecutionPolicy Bypass -File user-space/package.ps1

$ErrorActionPreference = 'Stop'
$Root = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $Root

Write-Host 'cargo build -p nav-app --release'
cargo build -p nav-app --release

$Us = Join-Path $Root 'user-space'
$Pub = Join-Path $Root 'public/screenshots'
$Shot = Join-Path $Us 'screenshots'

New-Item -ItemType Directory -Force -Path $Shot | Out-Null

Copy-Item (Join-Path $Root 'target/release/navigator.exe') (Join-Path $Us 'navigator.exe') -Force
Copy-Item (Join-Path $Root 'LICENSE') (Join-Path $Us 'LICENSE') -Force

if (Test-Path $Pub) {
    Get-ChildItem $Pub -File | Copy-Item -Destination $Shot -Force
}

$TomlLines = Get-Content (Join-Path $Root 'Cargo.toml')
$Ver = '1.0.0'
for ($i = 0; $i -lt $TomlLines.Length; $i++) {
    if ($TomlLines[$i] -match '^\[workspace\.package\]') {
        for ($j = $i + 1; $j -lt $TomlLines.Length; $j++) {
            if ($TomlLines[$j] -match '^\[') { break }
            if ($TomlLines[$j] -match '^version\s*=\s*"([^"]+)"') {
                $Ver = $Matches[1]
                break
            }
        }
        break
    }
}

$ZipName = "navigator-v${Ver}-windows-x86_64.zip"
$ZipPath = Join-Path $Us $ZipName

$Readme = Join-Path $Us 'README.txt'
if (-not (Test-Path $Readme)) {
    Write-Error "Missing $Readme"
}

if (Test-Path $ZipPath) {
    Remove-Item $ZipPath -Force
}

$itemsToZip = @(
    (Join-Path $Us 'navigator.exe'),
    $Readme,
    (Join-Path $Us 'LICENSE')
)
if (Test-Path $Shot) {
    $itemsToZip += $Shot
}

Compress-Archive -Path $itemsToZip -DestinationPath $ZipPath -Force
Write-Host "Created: $ZipPath"
