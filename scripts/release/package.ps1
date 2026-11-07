<#
.SYNOPSIS
    Builds the Windows release zip of Starry Journey.

.DESCRIPTION
    Builds starry-journey.exe in release mode with the C runtime linked in
    statically (so players need no Visual C++ redistributable), collects the
    licenses it must ship with, and zips everything as
    target\dist\starry-journey-<version>-windows-x64.zip.

    Builds go to target\dist, separate from target\ and target\agent, so
    packaging never rebuilds or locks a running development build.

    Requires cargo-about for the third-party license list:
        cargo install cargo-about --locked --features cli

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\release\package.ps1
#>

$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$here = $PSScriptRoot
$target = 'x86_64-pc-windows-msvc'
$distDir = Join-Path $root 'target\dist'

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$name = "starry-journey-$version-windows-x64"
$stage = Join-Path $distDir $name
$zip = Join-Path $distDir "$name.zip"

if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
    throw 'cargo-about is not installed. Run: cargo install cargo-about --locked --features cli'
}

Push-Location $root
try {
    # An explicit --target keeps RUSTFLAGS off build scripts and proc macros,
    # which must not be linked against the static C runtime.
    $env:CARGO_TARGET_DIR = $distDir
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    cargo build --release --locked --target $target --bin starry-journey
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
    Remove-Item Env:RUSTFLAGS

    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    if (Test-Path $zip) { Remove-Item -Force $zip }
    New-Item -ItemType Directory -Path $stage | Out-Null

    Copy-Item (Join-Path $distDir "$target\release\starry-journey.exe") $stage
    Copy-Item (Join-Path $root 'LICENSE') $stage
    Copy-Item (Join-Path $here 'README.txt') $stage
    Copy-Item (Join-Path $root 'src\fonts\NotoSans-OFL.txt') $stage
    Copy-Item (Join-Path $root 'src\fonts\unifont-OFL.txt') $stage

    cargo about generate -c (Join-Path $here 'about.toml') (Join-Path $here 'about.hbs') `
        -o (Join-Path $stage 'THIRD-PARTY-LICENSES.txt')
    if ($LASTEXITCODE -ne 0) { throw "cargo about failed with exit code $LASTEXITCODE" }

    Compress-Archive -Path $stage -DestinationPath $zip
    Write-Host "Built $zip"
}
finally {
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Pop-Location
}
