$ErrorActionPreference = 'Stop'
$assets = @(
    @('assets/fonts/SF-Pro-Text-Regular.otf', 'https://raw.githubusercontent.com/bradleyhodges/SFWindows/53cbc77150baa6222342aca231e4f55137b948af/SF%20Pro/SF-Pro-Text-Regular.otf', 'EABF0A67833BBE3C80C800E6CE3F36B65E92F3B6AC1D6226BA2981E5CB3A74A5'),
    @('assets/fonts/SF-Pro-Text-Semibold.otf', 'https://raw.githubusercontent.com/bradleyhodges/SFWindows/53cbc77150baa6222342aca231e4f55137b948af/SF%20Pro/SF-Pro-Text-Semibold.otf', '047AAED48E041497C75AE39329DEC6EE8456D8BFBBFAD4F5B67EBCD5867A2024'),
    @('assets/emoji/apple-32.png', 'https://raw.githubusercontent.com/iamcal/emoji-data/e77759c296ed37beb0dbd63908b43d8619509c6d/sheets-clean/sheet_apple_32_clean.png', 'C2392005858A5A9FAF58BC37B63450876F29EA54C291CA4E67729192EC0E631C')
)
foreach ($asset in $assets) {
    $path = Join-Path $PSScriptRoot $asset[0]
    if ((Test-Path -LiteralPath $path) -and (Get-FileHash -LiteralPath $path).Hash -eq $asset[2]) {
        Write-Output "OK: $($asset[0])"
        continue
    }
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    $temporary = $path + '.part'
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            Invoke-WebRequest -Uri $asset[1] -OutFile $temporary
            if ((Get-FileHash -LiteralPath $temporary).Hash -ne $asset[2]) { throw 'Asset checksum mismatch.' }
            Move-Item -LiteralPath $temporary -Destination $path -Force
            Write-Output "DOWNLOADED: $($asset[0])"
            break
        } catch {
            if ($attempt -eq 3) { throw }
        }
    }
}
