# Smoke test of the installer (release workflow, docs/specs/014-ci-release): installs silently into a temp folder,
# runs the installed app once with a screenshot, checks that it found its bundled helpers, then uninstalls and checks
# that the data folder stays.
#
#   ./packaging/smoke-test.ps1 -Installer target/dist/good-gallery-<version>-setup.exe [-DataDir <copy of an index>]
#
# Without -DataDir the app starts on an empty data folder. Never pass the real %APPDATA%\good-gallery: the app
# migrates and writes the index it opens.
param(
    [Parameter(Mandatory)] [string]$Installer,
    [string]$DataDir,
    [int]$ScreenshotAfter = 5
)

$ErrorActionPreference = 'Stop'

function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "Smoke test failed: $Message" }
    Write-Host "ok: $Message"
}

$Installer = (Resolve-Path $Installer).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) "gg-smoke-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
New-Item -ItemType Directory $work | Out-Null
$target = Join-Path $work 'Good Gallery'
if (-not $DataDir) { $DataDir = Join-Path $work 'data' }
$DataDir = [System.IO.Path]::GetFullPath($DataDir)
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Good Gallery.lnk'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\GoodGallery'

# /D must come last and unquoted (NSIS).
$install = Start-Process $Installer -ArgumentList '/S', "/D=$target" -Wait -PassThru
Assert ($install.ExitCode -eq 0) "the installer exits with 0 (got $($install.ExitCode))"
foreach ($file in 'good-gallery.exe', 'exiftool\exiftool.exe', 'exiftool\exiftool_files', 'ffmpeg\ffmpeg.exe',
    'mpv\libmpv-2.dll', 'LICENSE.txt', 'THIRD-PARTY-NOTICES.txt', 'Uninstall Good Gallery.exe') {
    Assert (Test-Path (Join-Path $target $file)) "installed $file"
}
Assert (Test-Path $uninstallKey) 'the uninstaller is registered'
Assert (Test-Path $shortcut) 'the Start menu shortcut is there'

$exe = Join-Path $target 'good-gallery.exe'
$shot = Join-Path $work 'screenshot.png'
$app = Start-Process $exe -PassThru -ArgumentList '--data-dir', "`"$DataDir`"", '--screenshot', "`"$shot`"",
    '--screenshot-after', $ScreenshotAfter
if (-not $app.WaitForExit(180000)) {
    $app.Kill()
    throw 'Smoke test failed: the app did not exit within 3 minutes'
}
Assert ($app.ExitCode -eq 0) "the app exits with 0 (got $($app.ExitCode))"
Assert (Test-Path $shot) 'the app saved its screenshot'
$log = Get-Content -Raw (Join-Path $DataDir 'logs\app.log')
# Only the end of the path: TEMP may be the 8.3 form (C:\Users\RUNNER~1\…) on one side and the long one on the other.
foreach ($helper in 'exiftool\exiftool.exe', 'ffmpeg\ffmpeg.exe', 'mpv\libmpv-2.dll') {
    $pattern = '(exiftool|ffmpeg|libmpv): .*\\Good Gallery\\' + [regex]::Escape($helper)
    Assert ($log -match $pattern) "the log finds the bundled $helper"
}
Assert (-not ($log -match ' ERROR ')) 'the log has no errors'

# _?= keeps the uninstaller in place and makes it wait (it copies itself to a temp folder otherwise).
$uninstaller = Join-Path $target 'Uninstall Good Gallery.exe'
$uninstall = Start-Process $uninstaller -ArgumentList '/S', "_?=$target" -Wait -PassThru
Assert ($uninstall.ExitCode -eq 0) "the uninstaller exits with 0 (got $($uninstall.ExitCode))"
Assert (-not (Test-Path $exe)) 'the app is removed'
Assert (-not (Test-Path (Join-Path $target 'exiftool'))) 'the helpers are removed'
Assert (-not (Test-Path $uninstallKey)) 'the uninstaller is unregistered'
Assert (-not (Test-Path $shortcut)) 'the Start menu shortcut is removed'
Assert (Test-Path (Join-Path $DataDir 'gallery.db')) 'the data folder stays'

Copy-Item $shot (Join-Path ([System.IO.Path]::GetTempPath()) 'gg-smoke-screenshot.png') -Force
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Write-Host 'Smoke test passed.'
