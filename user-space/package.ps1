#Requires -Version 5.1
# Build release navigator.exe, stage user-space/, create navigator-vVERSION-windows-x86_64.zip
# Zip contains only: navigator.exe, README.txt
# Run from repo root:  powershell -ExecutionPolicy Bypass -File user-space/package.ps1

$ErrorActionPreference = 'Stop'
$Root = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $Root

Write-Host 'cargo build -p nav-app --release'
cargo build -p nav-app --release

$Us = Join-Path $Root 'user-space'

Copy-Item (Join-Path $Root 'target/release/navigator.exe') (Join-Path $Us 'navigator.exe') -Force

$TomlLines = Get-Content (Join-Path $Root 'Cargo.toml')
$Ver = '1.1.0'
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
    $Readme
)

Compress-Archive -Path $itemsToZip -DestinationPath $ZipPath -Force
Write-Host "Created: $ZipPath"
