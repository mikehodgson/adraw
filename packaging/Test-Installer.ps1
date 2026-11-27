[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Installer,
    [Parameter(Mandatory)]
    [string]$PortableZip
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$registryKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B97FA13A-0923-4C24-A6D5-7F5276293F19}_is1'
$contextMenuKeys = @('.ans', '.asc') | ForEach-Object { "HKCU:\Software\Classes\SystemFileAssociations\$_\shell\ADraw" }
$openWithKeys = @('HKCU:\Software\Classes\ADraw.AnsiArt', 'HKCU:\Software\Classes\Applications\adraw.exe')
$openWithProgidKeys = @('.ans', '.asc') | ForEach-Object { "HKCU:\Software\Classes\$_\OpenWithProgids" }
function Test-OpenWithProgid([string]$Key) {
    (Test-Path -LiteralPath $Key) -and ((Get-Item -LiteralPath $Key).GetValueNames() -contains 'ADraw.AnsiArt')
}
if ((Test-Path -LiteralPath $registryKey) -or (($contextMenuKeys + $openWithKeys) | Where-Object { Test-Path -LiteralPath $_ }) -or
    ($openWithProgidKeys | Where-Object { Test-OpenWithProgid $_ })) {
    throw 'An ADraw installation already exists; do not run the smoke test against shared installer registration.'
}
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('adraw-installer-test-' + [guid]::NewGuid().ToString('N'))
$installDirectory = Join-Path $testRoot 'installed'
$portableDirectory = Join-Path $testRoot 'portable'
$uninstaller = Join-Path $installDirectory 'unins000.exe'
$installed = $false
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    Expand-Archive -LiteralPath $PortableZip -DestinationPath $portableDirectory
    $process = Start-Process -FilePath $Installer -ArgumentList @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/SP-', '/NORESTART', '/NOICONS',
        "/DIR=`"$installDirectory`"", "/LOG=`"$(Join-Path $testRoot 'install.log')`""
    ) -Wait -PassThru
    $installed = Test-Path -LiteralPath $uninstaller
    if ($process.ExitCode -ne 0 -or -not $installed) {
        throw "Installer smoke test failed (exit $($process.ExitCode))."
    }
    foreach ($file in @('adraw.exe', 'README.md', 'USER_GUIDE.md', 'CHANGELOG.md', 'NOTICE.txt', 'LICENSE.txt', 'LICENSE-IBM-VGA.txt', 'docs\images\adraw-screenshot.jpg', 'docs\images\gradient-direction.svg', 'docs\images\brush-tools.png', 'docs\images\colors-gradient.png', 'docs\images\minimap.png', 'examples\adraw-icon.ans')) {
        $copied = Get-FileHash -LiteralPath (Join-Path $installDirectory $file) -Algorithm SHA256
        $portable = Get-FileHash -LiteralPath (Join-Path $portableDirectory $file) -Algorithm SHA256
        if ($copied.Hash -ne $portable.Hash) { throw "Installer payload differs from ZIP: $file" }
    }
    $version = (Get-Item -LiteralPath (Join-Path $installDirectory 'adraw.exe')).VersionInfo.ProductVersion
    if ((Get-ItemProperty -LiteralPath $registryKey).DisplayVersion -ne $version) {
        throw 'Installer registration and executable product versions differ.'
    }
    $expectedCommand = '"{0}" -- "%1"' -f (Join-Path $installDirectory 'adraw.exe')
    foreach ($key in $contextMenuKeys) {
        if ((Get-ItemProperty -LiteralPath $key).'(default)' -ne 'Open with ADraw' -or
            (Get-ItemProperty -LiteralPath "$key\command").'(default)' -ne $expectedCommand) {
            throw "Context-menu registration is missing or incorrect: $key"
        }
    }
    foreach ($key in @('HKCU:\Software\Classes\ADraw.AnsiArt\shell\open\command', 'HKCU:\Software\Classes\Applications\adraw.exe\shell\open\command')) {
        if (-not (Test-Path -LiteralPath $key) -or (Get-ItemProperty -LiteralPath $key).'(default)' -ne $expectedCommand) {
            throw "Open with registration is missing or incorrect: $key"
        }
    }
    $supportedTypes = (Get-Item -LiteralPath 'HKCU:\Software\Classes\Applications\adraw.exe\SupportedTypes').GetValueNames()
    foreach ($extension in @('.ans', '.asc')) {
        if ($supportedTypes -notcontains $extension) { throw "Open with supported type is missing: $extension" }
    }
    foreach ($key in $openWithProgidKeys) {
        if (-not (Test-OpenWithProgid $key)) { throw "Open with ProgID is missing: $key" }
    }
    $artwork = Join-Path $installDirectory 'user-artwork.ans'
    Set-Content -LiteralPath $artwork -Value 'Saved artwork' -Encoding ascii
    $process = Start-Process -FilePath $uninstaller -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
    if ($process.ExitCode -ne 0 -or (Test-Path -LiteralPath (Join-Path $installDirectory 'adraw.exe')) -or (Test-Path -LiteralPath $registryKey)) {
        throw "Uninstaller smoke test failed (exit $($process.ExitCode))."
    }
    if ($contextMenuKeys | Where-Object { Test-Path -LiteralPath $_ }) {
        throw 'Uninstall left context-menu registration behind.'
    }
    if (($openWithKeys | Where-Object { Test-Path -LiteralPath $_ }) -or ($openWithProgidKeys | Where-Object { Test-OpenWithProgid $_ })) {
        throw 'Uninstall left Open with registration behind.'
    }
    if (-not (Test-Path -LiteralPath $artwork) -or (Get-Content -LiteralPath $artwork -Raw).Trim() -ne 'Saved artwork') {
        throw 'Uninstall removed or changed user artwork.'
    }
    $installed = $false
    Write-Host "Installer payload, version $version, context menus, Open with, installation, uninstallation, and artwork preservation verified."
}
finally {
    if ($installed -and (Test-Path -LiteralPath $uninstaller)) {
        $cleanup = Start-Process -FilePath $uninstaller -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
        if ($cleanup.ExitCode -ne 0) { throw "Test installation cleanup failed; inspect $testRoot." }
    }
    Remove-Item -LiteralPath $testRoot -Recurse -Force
}
