# Trail installer for Windows (PowerShell)
#
# Downloads the Windows release archive from GitHub, extracts it, and installs:
#   - trail.exe      (the binary)
#   - shell/trail.ps1 (the PowerShell cd-on-exit wrapper)
#
# Usage:
#   irm https://github.com/WeedonSctt/trail/releases/latest/download/install.ps1 | iex
#
# Or clone/download this script and run:
#   powershell -ExecutionPolicy Bypass -File install.ps1
#
# Parameters / environment variables:
#   -InstallDir   Where to install the binary.
#                 Default: $env:LOCALAPPDATA\trail\bin
#   -Version      Release version to install (without leading "v").
#                 Default: latest GitHub release.
#   -Repo         GitHub owner/repo.  Default: WeedonSctt/trail
#
# After installation:
#   Add the following line to your PowerShell profile ($PROFILE):
#
#     . "$env:LOCALAPPDATA\trail\bin\trail.ps1"
#
#   Then reload:  . $PROFILE
#
# The PowerShell wrapper is required for cd-on-exit.  trail.exe alone cannot
# change the parent shell's working directory.

[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "trail\bin"),
    [string]$Version    = "",
    [string]$Repo       = "WeedonSctt/trail"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

##############################################################################
# Helpers
##############################################################################

function Write-Info  { param([string]$msg) Write-Host "  info  $msg" -ForegroundColor Cyan    }
function Write-Ok    { param([string]$msg) Write-Host "    ok  $msg" -ForegroundColor Green   }
function Write-Warn  { param([string]$msg) Write-Host "  warn  $msg" -ForegroundColor Yellow  }
function Write-Err   { param([string]$msg) Write-Host " error  $msg" -ForegroundColor Red     }

function Exit-Error {
    param([string]$msg)
    Write-Err $msg
    exit 1
}

##############################################################################
# Resolve the version to install
##############################################################################

function Resolve-Version {
    if ($Version -ne "") {
        return $Version
    }

    Write-Info "Resolving latest release version from GitHub..."
    try {
        $response = Invoke-RestMethod `
            -Uri "https://api.github.com/repos/$Repo/releases/latest" `
            -UseBasicParsing
        $tag = $response.tag_name -replace '^v', ''
        if ([string]::IsNullOrWhiteSpace($tag)) {
            Exit-Error "Could not parse version from GitHub API response."
        }
        return $tag
    }
    catch {
        Exit-Error "Failed to resolve latest version: $_"
    }
}

##############################################################################
# Verify SHA-256 checksum
##############################################################################

function Verify-Checksum {
    param(
        [string]$FilePath,
        [string]$ArchiveName,
        [string]$ChecksumsUrl
    )

    Write-Info "Downloading checksums for verification..."
    try {
        # GitHub serves release assets as application/octet-stream, and for a
        # non-text content type Invoke-WebRequest hands back .Content as a
        # System.Byte[] rather than a string. Running -match against a byte
        # array never matches, which silently routes every install into the
        # "skipping verification" branch below — i.e. the integrity check
        # quietly does nothing. Decode explicitly instead of trusting the type.
        $raw = (Invoke-WebRequest -Uri $ChecksumsUrl -UseBasicParsing).Content
        $checksumContent = if ($raw -is [byte[]]) {
            [System.Text.Encoding]::UTF8.GetString($raw)
        } else {
            [string]$raw
        }
        if ($checksumContent -match "([a-fA-F0-9]{64})\s+$([regex]::Escape($ArchiveName))") {
            $expected = $Matches[1].ToLower()
            $actual   = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash.ToLower()
            if ($actual -ne $expected) {
                Exit-Error @"
SHA-256 checksum mismatch for $ArchiveName!
  Expected: $expected
  Actual:   $actual
The download may be corrupted. Delete the temp file and retry.
"@
            }
            Write-Ok "Checksum verified"
        }
        else {
            Write-Warn "$ArchiveName not found in checksums.txt; skipping verification."
        }
    }
    catch {
        Write-Warn "Could not download checksums.txt; skipping verification."
    }
}

##############################################################################
# Add a directory to the user-scope PATH (idempotent)
##############################################################################

function Add-ToUserPath {
    param([string]$Dir)

    $currentPath = [System.Environment]::GetEnvironmentVariable("PATH", "User")
    $parts = $currentPath -split ";"

    if ($parts -notcontains $Dir) {
        $newPath = ($parts + $Dir) -join ";"
        [System.Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
        # Also update the current session's PATH.
        $env:PATH = "$env:PATH;$Dir"
        Write-Ok "Added $Dir to user PATH"
    }
}

##############################################################################
# Main
##############################################################################

$version     = Resolve-Version
$target      = "x86_64-pc-windows-msvc"
$archiveName = "trail-v$version-$target.zip"
$archiveUrl  = "https://github.com/$Repo/releases/download/v$version/$archiveName"
$checksumUrl = "https://github.com/$Repo/releases/download/v$version/checksums.txt"

Write-Info "Installing trail v$version for Windows (x86_64)"
Write-Info "Downloading $archiveUrl"

# Work in a temporary directory that is cleaned up on exit.
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("trail-install-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpDir | Out-Null

try {
    $archivePath = Join-Path $tmpDir $archiveName

    # Download the archive.
    try {
        Invoke-WebRequest -Uri $archiveUrl -OutFile $archivePath -UseBasicParsing
    }
    catch {
        Exit-Error "Download failed: $_"
    }

    # Verify checksum.
    Verify-Checksum -FilePath $archivePath -ArchiveName $archiveName -ChecksumsUrl $checksumUrl

    # Extract the archive.
    Expand-Archive -Path $archivePath -DestinationPath $tmpDir -Force
    $extractedDir = Join-Path $tmpDir "trail-v$version-$target"

    if (-not (Test-Path $extractedDir)) {
        Exit-Error "Extracted directory not found: $extractedDir"
    }

    # Create the install directory.
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    # Install the binary.
    $binaryDest = Join-Path $InstallDir "trail.exe"
    Copy-Item -Path (Join-Path $extractedDir "trail.exe") -Destination $binaryDest -Force
    Write-Ok "Binary installed to $binaryDest"

    # Install the PowerShell wrapper (shell/trail.ps1).
    # The wrapper is placed alongside the binary so that the profile snippet
    # `. "$InstallDir\trail.ps1"` works after a single PATH addition.
    $wrapperDest = Join-Path $InstallDir "trail.ps1"
    $shellDir    = Join-Path $extractedDir "shell"
    Copy-Item -Path (Join-Path $shellDir "trail.ps1") -Destination $wrapperDest -Force
    Write-Ok "PowerShell wrapper installed to $wrapperDest"

    # Also install the POSIX wrappers alongside the binary for users who run
    # bash/zsh/fish inside WSL or Git Bash alongside PowerShell.
    foreach ($wrapper in @("trail.bash", "trail.zsh", "trail.fish")) {
        $src = Join-Path $shellDir $wrapper
        if (Test-Path $src) {
            Copy-Item -Path $src -Destination (Join-Path $InstallDir $wrapper) -Force
        }
    }

    # Add InstallDir to the user-scope PATH.
    Add-ToUserPath -Dir $InstallDir
}
finally {
    # Always clean up the temp directory.
    if (Test-Path $tmpDir) {
        Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Info "Installation complete!"
Write-Host ""
Write-Host "  To enable cd-on-exit, add the following line to your PowerShell profile."
Write-Host "  Your profile path is: $PROFILE"
Write-Host ""
Write-Host "    . `"$InstallDir\trail.ps1`""
Write-Host ""
Write-Host "  Then reload your profile:"
Write-Host ""
Write-Host "    . `$PROFILE"
Write-Host ""
Write-Host "  After that, running 'trail' will use the wrapper and your session"
Write-Host "  will cd to the last-browsed directory when you quit normally."
Write-Host ""
