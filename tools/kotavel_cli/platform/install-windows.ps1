param(
    [Parameter(Mandatory=$true)][string]$Tools,
    [Parameter(Mandatory=$true)][string]$Compiler,
    [Parameter(Mandatory=$true)][string]$Architecture,
    [Parameter(Mandatory=$true)][string]$Manifest,
    [switch]$ConfigureOnly
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Este instalador requiere Windows.' }
$compilerBin = ''
if ($Compiler -eq 'clang') {
    if ($Architecture -ne 'x86_64') { throw 'LLVM-MinGW portable requiere Python x64.' }
    $toolchain = Get-ChildItem -LiteralPath $Tools -Directory -Filter 'llvm-mingw-*-ucrt-x86_64' | Where-Object { Test-Path (Join-Path $_.FullName 'bin/clang++.exe') } | Sort-Object Name -Descending | Select-Object -First 1
    if (-not $toolchain -and -not $ConfigureOnly) {
        $release = Invoke-RestMethod 'https://api.github.com/repos/mstorsjo/llvm-mingw/releases/latest' -TimeoutSec 60
        $asset = $release.assets | Where-Object name -Match '^llvm-mingw-.*-ucrt-x86_64.zip$' | Select-Object -First 1
        if (-not $asset) { throw 'No se encontró LLVM-MinGW para Windows x64.' }
        $archive = Join-Path $Tools 'llvm-mingw.zip'
        Write-Host 'Descargando el compilador LLVM-MinGW...'
        Invoke-WebRequest $asset.browser_download_url -OutFile $archive -UseBasicParsing -TimeoutSec 180
        Expand-Archive -LiteralPath $archive -DestinationPath $Tools -Force
        $toolchain = Get-ChildItem -LiteralPath $Tools -Directory -Filter 'llvm-mingw-*-ucrt-x86_64' | Where-Object { Test-Path (Join-Path $_.FullName 'bin/clang++.exe') } | Sort-Object Name -Descending | Select-Object -First 1
    }
    if (-not $toolchain) { throw 'Falta LLVM-MinGW; ejecuta el instalador sin --configure-only.' }
    $compilerBin = Join-Path $toolchain.FullName 'bin'
    $env:PATH = "$compilerBin;$env:PATH"
}
$shell = Get-Command pwsh -ErrorAction SilentlyContinue
if (-not $shell) { $shell = Get-Command powershell -ErrorAction Stop }
@{
    scripts_directory = 'Scripts'
    python_executable = 'python.exe'
    compiler_bin = $compilerBin
    shell = @($shell.Source, '-NoLogo', '-NoProfile')
    activation_kind = 'powershell'
    build_environment = $(if ($Compiler -eq 'clang') { @{ CONAN_OPENSSL_CONFIGURATION = 'mingw64' } } else { @{} })
} | ConvertTo-Json | Set-Content -LiteralPath $Manifest -Encoding UTF8
