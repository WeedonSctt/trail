# Trail uninstaller for Windows (PowerShell)
#
# Removes what install.ps1 installed: the binary, the shell wrappers, and the
# user-scope PATH entry. Configuration and data (bookmarks, recent
# directories, the remembered --config path) are LEFT ALONE unless you pass
# -Purge.
#
# Usage:
#   irm https://github.com/WeedonSctt/trail/releases/latest/download/uninstall.ps1 | iex
#
# Or download this script and run:
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1 -Purge
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1 -DryRun
#
# Parameters:
#   -InstallDir   Where the binary was installed.
#                 Default: $env:LOCALAPPDATA\trail\bin
#   -Purge        Also remove configuration and data ($env:APPDATA\trail).
#   -DryRun       Print what would be removed, change nothing.
#
# This script does NOT edit your PowerShell profile. It reports the line that
# dot-sources the wrapper so you can remove it, because silently rewriting
# $PROFILE is not a thing an uninstaller should do unasked.
#
# KEEP THIS FILE PURE ASCII. Windows PowerShell 5.1 - which is what
# `irm ... | iex` runs on a stock Windows box - decodes a BOM-less file as
# ANSI, not UTF-8. A UTF-8 em dash then decodes to a trailing byte 0x94, which
# CP1252 maps to a right curly quote, and PowerShell accepts curly quotes as
# string delimiters. One em dash inside one string therefore desynchronizes
# the parser and the whole script fails to load.

[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "trail\bin"),
    [switch]$Purge,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

##############################################################################
# Helpers
##############################################################################

function Write-Info { param([string]$msg) Write-Host "  info  $msg" -ForegroundColor Cyan   }
function Write-Ok   { param([string]$msg) Write-Host "    ok  $msg" -ForegroundColor Green  }
function Write-Warn { param([string]$msg) Write-Host "  warn  $msg" -ForegroundColor Yellow }
function Write-Kept { param([string]$msg) Write-Host "  kept  $msg" -ForegroundColor Cyan   }

# Removes a file or directory, honouring -DryRun. An absent target is not an
# error: a partial install is exactly the case an uninstaller must handle.
function Remove-Target {
    param([string]$Path, [switch]$Recurse)

    if (-not (Test-Path -LiteralPath $Path)) { return }

    if ($DryRun) {
        Write-Info "would remove  $Path"
        return
    }

    try {
        Remove-Item -LiteralPath $Path -Force -Recurse:$Recurse
        Write-Ok "removed  $Path"
    }
    catch {
        # The overwhelmingly likely cause is a running trail.exe: Windows will
        # not unlink an executable that is mapped into a live process.
        Write-Warn "could not remove $Path - $($_.Exception.Message)"
        Write-Warn "If Trail is running, quit it and re-run this script."
    }
}

##############################################################################
# Refuse to fight a package manager
##############################################################################

# Deleting files out from under Scoop leaves it believing trail is still
# installed, which breaks its next update. Detect it and hand over the right
# command instead.
$resolved = (Get-Command trail -ErrorAction SilentlyContinue).Source
if ($resolved) {
    if ($env:SCOOP -and $resolved.StartsWith($env:SCOOP, [System.StringComparison]::OrdinalIgnoreCase)) {
        Write-Warn "trail at $resolved was installed by Scoop."
        Write-Warn "Use:  scoop uninstall trail"
        exit 1
    }
    if ($resolved -like "*\.cargo\bin\trail.exe") {
        Write-Warn "trail at $resolved was installed by cargo."
        Write-Warn "Use:  cargo uninstall trail"
        exit 1
    }
}

##############################################################################
# Main
##############################################################################

if ($DryRun) {
    Write-Info "Dry run - nothing will be removed."
}

Write-Info "Removing Trail"

# The binary and every wrapper install.ps1 places beside it.
Remove-Target -Path (Join-Path $InstallDir "trail.exe")
foreach ($wrapper in @("trail.ps1", "trail.bash", "trail.zsh", "trail.fish")) {
    Remove-Target -Path (Join-Path $InstallDir $wrapper)
}

# Remove the install directory itself, but only if nothing else is in it.
if ((Test-Path -LiteralPath $InstallDir) -and -not $DryRun) {
    if (-not (Get-ChildItem -LiteralPath $InstallDir -Force)) {
        Remove-Item -LiteralPath $InstallDir -Force
        Write-Ok "removed  $InstallDir"
    }
    else {
        Write-Kept "$InstallDir  (not empty - left in place)"
    }
}

##############################################################################
# PATH entry
##############################################################################

# Read and write the user PATH through the registry rather than
# [Environment]::SetEnvironmentVariable. That API writes REG_SZ and expands
# any %VAR% references already present, which permanently flattens entries
# like %JAVA_HOME%\bin in the user's PATH. Going through the registry key
# directly preserves both the value kind and the unexpanded references.
function Remove-FromUserPath {
    param([string]$Dir)

    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
    if (-not $key) {
        Write-Warn "Could not open HKCU\Environment; remove $Dir from PATH by hand."
        return
    }

    try {
        $kind = $key.GetValueKind("PATH")
        $current = $key.GetValue(
            "PATH", "",
            [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)

        $parts = @($current -split ";" | Where-Object { $_ -ne "" })
        $kept  = @($parts | Where-Object { $_.TrimEnd('\') -ne $Dir.TrimEnd('\') })

        if ($parts.Count -eq $kept.Count) {
            return  # not on PATH; nothing to do
        }

        if ($DryRun) {
            Write-Info "would remove from user PATH  $Dir"
            return
        }

        $key.SetValue("PATH", ($kept -join ";"), $kind)
        Write-Ok "removed from user PATH  $Dir"
        Write-Info "Open a new terminal for the PATH change to take effect."
    }
    catch {
        Write-Warn "Could not update PATH - $($_.Exception.Message)"
        Write-Warn "Remove $Dir from your user PATH by hand."
    }
    finally {
        $key.Close()
    }
}

Remove-FromUserPath -Dir $InstallDir

##############################################################################
# Config and data - only with -Purge
##############################################################################

# The `directories` crate puts both under %APPDATA%\trail on Windows:
# ...\trail\config and ...\trail\data. `trail --paths` prints the exact
# values, and is authoritative if this script and the binary disagree.
$appData = Join-Path $env:APPDATA "trail"

if ($Purge) {
    Write-Warn "-Purge: removing configuration and data"
    Remove-Target -Path $appData -Recurse
}
elseif (Test-Path -LiteralPath $appData) {
    Write-Kept "$appData  (config, bookmarks, recent dirs - remove with -Purge)"
}

# The log is a debugging artefact in the temp directory, not state.
Remove-Target -Path (Join-Path ([System.IO.Path]::GetTempPath()) "trail.log")

##############################################################################
# Profile line - reported, never edited
##############################################################################

# The two PowerShell editions keep their profiles in different directories
# under the same Documents root - WindowsPowerShell\ for 5.1, PowerShell\ for
# 7+ - and a user may well have added the wrapper line under one edition while
# running this script under the other. Checking only $PROFILE would then miss
# the line and leave every new session erroring on a file that no longer
# exists, which is the failure this whole section exists to prevent.
#
# The Documents root is derived from $PROFILE rather than assumed, because it
# is routinely redirected to OneDrive and $env:USERPROFILE\Documents is then
# the wrong answer.
function Get-ProfileCandidates {
    $roots = @()
    foreach ($known in @($PROFILE.CurrentUserCurrentHost, $PROFILE.CurrentUserAllHosts)) {
        if (-not $known) { continue }
        $editionDir = Split-Path -Parent $known       # ...\Documents\PowerShell
        $documents  = Split-Path -Parent $editionDir  # ...\Documents
        if ($documents) { $roots += $documents }
    }

    $candidates = @()
    foreach ($root in ($roots | Select-Object -Unique)) {
        foreach ($edition in @("WindowsPowerShell", "PowerShell")) {
            foreach ($name in @("Microsoft.PowerShell_profile.ps1", "profile.ps1")) {
                $candidates += (Join-Path (Join-Path $root $edition) $name)
            }
        }
    }

    $candidates | Select-Object -Unique
}

$reported = $false
foreach ($profilePath in (Get-ProfileCandidates)) {
    if (-not $profilePath -or -not (Test-Path -LiteralPath $profilePath)) { continue }

    $hits = Select-String -LiteralPath $profilePath -Pattern "trail\.ps1" -ErrorAction SilentlyContinue
    if (-not $hits) { continue }

    if (-not $reported) {
        Write-Host ""
        Write-Warn "This line still dot-sources the Trail wrapper. Remove it by hand,"
        Write-Warn "or every new PowerShell session will report a missing file:"
        Write-Host ""
        $reported = $true
    }
    Write-Host "    $profilePath"
    foreach ($hit in $hits) {
        Write-Host "      $($hit.LineNumber): $($hit.Line.Trim())"
    }
}

Write-Host ""
if ($DryRun) {
    Write-Info "Dry run complete. Re-run without -DryRun to apply."
}
else {
    Write-Info "Trail has been removed."
}
Write-Host ""
