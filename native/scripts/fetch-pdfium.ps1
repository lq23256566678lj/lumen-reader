param(
    [string]$Destination = (Join-Path $PSScriptRoot '..\pdfium')
)

$ErrorActionPreference = 'Stop'
$archiveUrl = 'https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.tgz'
$tempRoot = Join-Path $env:TEMP ('lumen-pdfium-' + [Guid]::NewGuid().ToString('N'))
$archive = Join-Path $tempRoot 'pdfium.tgz'

New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
try {
    Write-Host "Downloading $archiveUrl"
    Invoke-WebRequest -Uri $archiveUrl -OutFile $archive -UseBasicParsing
    tar -xzf $archive -C $tempRoot
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Copy-Item (Join-Path $tempRoot 'bin\pdfium.dll') (Join-Path $Destination 'pdfium.dll') -Force
    Write-Host "Installed $Destination\pdfium.dll"
}
finally {
    Remove-Item -Recurse -Force $tempRoot -ErrorAction SilentlyContinue
}
