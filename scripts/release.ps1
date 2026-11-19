# Prepare a release: set the version in Cargo.toml and the installer script, build rview.exe,
# regenerate THIRD-PARTY-NOTICES.txt and, with Inno Setup 6, build the installer locally.
# Then commit, push, and push the tag v<Version>: .github/workflows/build.yml checks that the tag
# matches Cargo.toml, builds rview.exe and publishes the GitHub release with the exe and the notices.
#   powershell -ExecutionPolicy Bypass -File scripts\release.ps1 1.2.0
param([Parameter(Mandatory = $true)][string]$Version)

$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must look like 1.2.0, got '$Version'" }
$root = Split-Path -Parent $PSScriptRoot
$utf8 = New-Object System.Text.UTF8Encoding $false

function Set-Line([string]$file, [string]$pattern, [string]$line) {
    $path = Join-Path $root $file
    $text = [IO.File]::ReadAllText($path)
    $re = [regex]::new($pattern, 'Multiline')
    if (-not $re.IsMatch($text)) { throw "$file has no line matching $pattern" }
    [IO.File]::WriteAllText($path, $re.Replace($text, $line, 1), $utf8)
}

Set-Line 'Cargo.toml' '^version = "[^"]*"' "version = `"$Version`""
Set-Line 'dist\win\rview.iss' '^#define AppVersion "[^"]*"' "#define AppVersion `"$Version`""

Push-Location $root
try {
    cargo build --release -p rview
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    python scripts\third_party_notices.py
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    if (-not (Test-Path $iscc)) { $iscc = "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" }
    if (Test-Path $iscc) {
        & $iscc /Q dist\win\rview.iss
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        Write-Host "rView ${Version}: target\release\rview.exe, target\installer\rview-$Version-setup-x64.exe"
    } else {
        Write-Host "rView ${Version}: target\release\rview.exe (Inno Setup 6 not found, no installer)"
    }
    Write-Host "Commit Cargo.toml, Cargo.lock, dist\win\rview.iss and THIRD-PARTY-NOTICES.txt, push, then"
    Write-Host "  git tag v$Version; git push origin v$Version"
    Write-Host "CI builds rview.exe and publishes the v$Version release."
} finally {
    Pop-Location
}
