[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$InnoCompiler,
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '..\target\dist')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if (-not (Test-Path -LiteralPath $InnoCompiler -PathType Leaf)) {
    throw 'ISCC.exe was not found. Run packaging\Get-InnoSetup.ps1 first.'
}
$oldRustFlags = $env:RUSTFLAGS
$stage = $null
Push-Location $root
try {
    $metadata = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
    $package = ($metadata | ConvertFrom-Json).packages | Where-Object name -EQ 'adraw'
    $version = $package.version
    if ($version -notmatch '^(\d+\.\d+\.\d+)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$') {
        throw "Unsupported release version: $version"
    }
    $fileVersion = "$($Matches[1]).0"
    if ($env:GITHUB_REF_TYPE -eq 'tag' -and $env:GITHUB_REF_NAME -cne "v$version" -and $env:GITHUB_REF_NAME -cne $version) {
        throw "Tag $env:GITHUB_REF_NAME does not match Cargo version $version."
    }

    $env:RUSTFLAGS = "$oldRustFlags -C target-feature=+crt-static".Trim()
    & cargo build --locked --release -p adraw --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'The Windows release build failed.' }

    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
    $stage = Join-Path $OutputDirectory ('.stage-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage | Out-Null
    Copy-Item -LiteralPath (Join-Path $root 'target\x86_64-pc-windows-msvc\release\adraw.exe') -Destination $stage
    foreach ($doc in @('README.md', 'USER_GUIDE.md', 'CHANGELOG.md', 'NOTICE.txt')) {
        Copy-Item -LiteralPath (Join-Path $root $doc) -Destination $stage
    }
    Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination (Join-Path $stage 'LICENSE.txt')
    Copy-Item -LiteralPath (Join-Path $root 'crates\ansi_editor\assets\LICENSE-IBM-VGA.txt') -Destination $stage
    foreach ($asset in @('docs\images\adraw-screenshot.jpg', 'examples\adraw-icon.ans')) {
        $destination = Join-Path $stage $asset
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $root $asset) -Destination $destination
    }

    $zip = Join-Path $OutputDirectory "adraw-$version-windows-x64.zip"
    Compress-Archive -LiteralPath @(Get-ChildItem -LiteralPath $stage | Select-Object -ExpandProperty FullName) -DestinationPath $zip -Force
    & $InnoCompiler "/DAppVersion=$version" "/DFileVersion=$fileVersion" "/DSourceDir=$stage" "/DOutputDir=$OutputDirectory" "/DProjectRoot=$root" (Join-Path $PSScriptRoot 'adraw.iss')
    if ($LASTEXITCODE -ne 0) { throw 'The Windows installer build failed.' }

    $installer = Join-Path $OutputDirectory "adraw-$version-windows-x64-setup.exe"
    foreach ($artifact in @($installer, $zip)) {
        if (-not (Test-Path -LiteralPath $artifact)) { throw "Missing release artifact: $artifact" }
    }
    @($installer, $zip) | ForEach-Object {
        "$((Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant())  $(Split-Path -Leaf $_)"
    } | Set-Content -LiteralPath (Join-Path $OutputDirectory 'SHA256SUMS.txt') -Encoding ascii
    Write-Host "Release $version is ready in $OutputDirectory"
}
finally {
    $env:RUSTFLAGS = $oldRustFlags
    if ($stage -and (Test-Path -LiteralPath $stage)) {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
    Pop-Location
}
