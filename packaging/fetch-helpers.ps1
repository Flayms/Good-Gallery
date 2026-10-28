# Downloads the helper programs the app bundles and unpacks them in the layout it looks for next to its executable
# (media::helpers): exiftool\exiftool.exe (+ exiftool_files), ffmpeg\ffmpeg.exe, mpv\libmpv-2.dll.
# Every download is checked against a pinned SHA-256; downloads are kept in <Out>\downloads and reused.
#
#   ./packaging/fetch-helpers.ps1 [-Out target/helpers]
#
# Then `cargo xtask dist` packages them, and tests or `cargo run` can use them through GOOD_GALLERY_EXIFTOOL,
# GOOD_GALLERY_FFMPEG and GOOD_GALLERY_MPV. Pinned versions: exiftool 13.59 (the exiftool-vendored.exe build),
# ffmpeg-static b6.1.1, libmpv 20261021 (shinchiro).
param(
    [string]$Out = (Join-Path $PSScriptRoot '..\target\helpers')
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$helpers = @(
    @{
        Name   = 'exiftool 13.59'
        File   = 'exiftool-13.59_64.zip'
        Urls   = @('https://sourceforge.net/projects/exiftool/files/exiftool-13.59_64.zip/download')
        Sha256 = '44b512b25af500724ba579d0a53c8fc5851628b692dd5e5d94ae4a15c2cba9ec'
    },
    @{
        Name   = 'ffmpeg 6.1.1 (ffmpeg-static b6.1.1)'
        File   = 'ffmpeg-win32-x64.gz'
        Urls   = @('https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-win32-x64.gz')
        Sha256 = '8883a3dffbd0a16cf4ef95206ea05283f78908dbfb118f73c83f4951dcc06d77'
    },
    @{
        Name   = 'libmpv (shinchiro mpv-winbuild-cmake 20261021)'
        File   = 'mpv-dev-x86_64-20261021-git-e471496c56.7z'
        # shinchiro removes older GitHub releases; SourceForge keeps them.
        Urls   = @(
            'https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/20261021/mpv-dev-x86_64-20261021-git-e471496c56.7z',
            'https://sourceforge.net/projects/mpv-player-windows/files/libmpv/mpv-dev-x86_64-20261021-git-e471496c56.7z/download'
        )
        Sha256 = 'efc719eff3dc5954b4a5efc800632808bcec01cc9034940dd14e5eb7fe9ea633'
    }
)

function Test-Hash([string]$Path, [string]$Expected) {
    (Test-Path $Path) -and ((Get-FileHash -Algorithm SHA256 $Path).Hash -eq $Expected.ToUpperInvariant())
}

function Get-Download($helper, [string]$Folder) {
    $path = Join-Path $Folder $helper.File
    if (Test-Hash $path $helper.Sha256) {
        return $path
    }
    foreach ($url in $helper.Urls) {
        Write-Host "Downloading $($helper.Name) from $url"
        try {
            Invoke-WebRequest -Uri $url -OutFile $path -UserAgent 'curl/8' -MaximumRedirection 10
        } catch {
            Write-Warning "  $($_.Exception.Message)"
            continue
        }
        if (Test-Hash $path $helper.Sha256) {
            return $path
        }
        Write-Warning "  SHA-256 mismatch: $((Get-FileHash -Algorithm SHA256 $path).Hash)"
        Remove-Item $path
    }
    throw "Couldn't download $($helper.Name) with the expected SHA-256 $($helper.Sha256)."
}

# Windows' own tar (bsdtar) reads zip and 7z; Git Bash's GNU tar on PATH doesn't.
$tar = Join-Path $env:SystemRoot 'System32\tar.exe'

function Expand-With-Tar([string]$Archive, [string]$Destination, [string[]]$Options = @()) {
    New-Item -ItemType Directory -Force $Destination | Out-Null
    & $tar -xf $Archive -C $Destination @Options
    if ($LASTEXITCODE -ne 0) { throw "tar couldn't unpack $Archive" }
}

$Out = [System.IO.Path]::GetFullPath($Out)
$downloads = Join-Path $Out 'downloads'
New-Item -ItemType Directory -Force $downloads | Out-Null

# exiftool: the zip's top folder is dropped and its exiftool(-k).exe (which waits for a key press when started by
# double-click) becomes exiftool.exe; it needs exiftool_files next to it. Copied, not renamed: Defender holds freshly
# written executables open for a moment, so renaming or moving them fails with "access denied".
$archive = Get-Download $helpers[0] $downloads
$exiftoolDir = Join-Path $Out 'exiftool'
Remove-Item -Recurse -Force $exiftoolDir -ErrorAction SilentlyContinue
Expand-With-Tar $archive $exiftoolDir @('--strip-components', '1')
$launcher = Join-Path $exiftoolDir 'exiftool(-k).exe'
Copy-Item $launcher (Join-Path $exiftoolDir 'exiftool.exe')
foreach ($attempt in 1..20) {
    try { Remove-Item $launcher; break } catch { Start-Sleep -Milliseconds 500 }
}
if (Test-Path $launcher) { throw "Couldn't remove $launcher" }

# ffmpeg: one gzipped executable.
$archive = Get-Download $helpers[1] $downloads
$ffmpegDir = Join-Path $Out 'ffmpeg'
New-Item -ItemType Directory -Force $ffmpegDir | Out-Null
$source = [System.IO.File]::OpenRead($archive)
try {
    $gzip = [System.IO.Compression.GZipStream]::new($source, [System.IO.Compression.CompressionMode]::Decompress)
    $target = [System.IO.File]::Create((Join-Path $ffmpegDir 'ffmpeg.exe'))
    try { $gzip.CopyTo($target) } finally { $target.Dispose(); $gzip.Dispose() }
} finally {
    $source.Dispose()
}

# libmpv: only the DLL from the development package.
$archive = Get-Download $helpers[2] $downloads
$mpvDir = Join-Path $Out 'mpv'
Remove-Item -Recurse -Force $mpvDir -ErrorAction SilentlyContinue
Expand-With-Tar $archive $mpvDir @('libmpv-2.dll')

foreach ($file in 'exiftool\exiftool.exe', 'exiftool\exiftool_files', 'ffmpeg\ffmpeg.exe', 'mpv\libmpv-2.dll') {
    if (-not (Test-Path (Join-Path $Out $file))) { throw "$file is missing after unpacking" }
}
Write-Host "Helpers in $Out"
