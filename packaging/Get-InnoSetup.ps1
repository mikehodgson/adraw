[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Destination
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Destination = [IO.Path]::GetFullPath($Destination)
$compilerDirectory = Join-Path $Destination 'compiler'
$compiler = Join-Path $compilerDirectory 'ISCC.exe'
if (Test-Path -LiteralPath $compiler) {
    return $compiler
}

New-Item -ItemType Directory -Path $Destination -Force | Out-Null
$setup = Join-Path $Destination 'innosetup-6.7.4.exe'
Invoke-WebRequest -Uri 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_4/innosetup-6.7.4.exe' -OutFile $setup
$expectedHash = 'a549929319f9120716677c685535ca4671e4b937b5fdc87e2585f32d8eb30e8c'
if ((Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedHash) {
    throw 'The Inno Setup download does not match its pinned SHA-256 checksum.'
}
$signature = Get-AuthenticodeSignature -LiteralPath $setup
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike '*Pyrsys B.V.*') {
    throw 'The Inno Setup download does not have a valid Pyrsys B.V. signature.'
}

$process = Start-Process -FilePath $setup -ArgumentList @(
    '/PORTABLE=1', '/CURRENTUSER', '/VERYSILENT', '/SUPPRESSMSGBOXES',
    '/SP-', '/NORESTART', '/NOICONS', '/TASKS=""',
    "/DIR=`"$compilerDirectory`""
) -Wait -PassThru
if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $compiler)) {
    throw "Portable Inno Setup installation failed (exit $($process.ExitCode))."
}
$compiler
