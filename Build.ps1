$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'target\release\iphone-bridge.exe') -Destination (Join-Path $PSScriptRoot 'iMessage.exe')
    Write-Output 'BUILD_OK: iMessage.exe'
} finally { Pop-Location }
