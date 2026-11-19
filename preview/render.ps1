# Render every page in preview\src into preview\renders\wide (1920x1080) and preview\renders\square
# (1200x1200) with headless Edge. Set EDGE_PATH if Edge is installed somewhere else.
#   powershell -ExecutionPolicy Bypass -File preview\render.ps1
$ErrorActionPreference = 'Stop'
$edge = if ($env:EDGE_PATH) { $env:EDGE_PATH } else { "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe" }
if (-not (Test-Path $edge)) { throw "Edge not found at $edge; set EDGE_PATH" }

$sizes = @{ wide = '1920,1080'; square = '1200,1200' }
foreach ($layout in $sizes.Keys) {
    $out = Join-Path $PSScriptRoot "renders\$layout"
    New-Item -ItemType Directory -Force $out | Out-Null
    foreach ($page in Get-ChildItem (Join-Path $PSScriptRoot 'src') -Filter *.html) {
        $png = Join-Path $out ($page.BaseName + '.png')
        $url = ([Uri]$page.FullName).AbsoluteUri
        $edgeArgs = @('--headless=new', '--disable-gpu', '--hide-scrollbars', '--force-device-scale-factor=1',
            '--allow-file-access-from-files', '--virtual-time-budget=3000', "--window-size=$($sizes[$layout])",
            "--screenshot=$png", $url)
        $p = Start-Process $edge $edgeArgs -Wait -PassThru -WindowStyle Hidden
        if ($p.ExitCode -ne 0 -or -not (Test-Path $png)) { throw "Edge failed on $($page.Name)" }
        Write-Host "renders\$layout\$($page.BaseName).png"
    }
}
