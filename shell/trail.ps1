# PowerShell wrapper for the `trail` terminal file manager.
#
# Add this to your PowerShell profile to enable the "cd-on-exit" behaviour:
# when you quit Trail normally (`q`), your current shell session changes to
# the directory Trail was browsing.
#
# To find your profile path, run: $PROFILE
#
# Installation:
#   Add the following line to your $PROFILE:
#   . "/path/to/trail.ps1"
#
# Usage:
#   trail [<start-path>]      # launches Trail; on normal exit, changes the
#                             # location to the last-visited directory.
#
# How it works:
#   The wrapper creates a temporary file and passes its path to the Trail
#   binary via --cwd-file. On a normal exit Trail writes its current
#   directory to that file; the wrapper reads it, removes it, then changes
#   the location to the directory. On cancellation (Ctrl-c / Esc quit) Trail writes
#   nothing, so the shell stays where it was.

function trail {
    $tmp = [System.IO.Path]::GetTempFileName()
    
    # We must invoke the actual application, not this function, to avoid recursion.
    $trailApp = Get-Command trail -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $trailApp) {
        Write-Error "trail executable not found in PATH."
        return
    }

    try {
        & $trailApp.Path --cwd-file $tmp $args
    } finally {
        $exitCode = $LASTEXITCODE
        
        if (Test-Path -LiteralPath $tmp) {
            $dir = (Get-Content -LiteralPath $tmp -Raw)
            if ($null -ne $dir) {
                $dir = $dir.Trim()
            }
            
            Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
            
            if (-not [string]::IsNullOrWhiteSpace($dir) -and (Test-Path -LiteralPath $dir -PathType Container)) {
                Set-Location -LiteralPath $dir
            }
        }
        
        $global:LASTEXITCODE = $exitCode
    }
}
