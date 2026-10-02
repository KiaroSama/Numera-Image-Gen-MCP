#Requires -Version 7.0
[CmdletBinding(SupportsShouldProcess)]
param([Parameter(Mandatory)][string]$Path)
. (Join-Path $PSScriptRoot 'common.ps1')
Initialize-NumeraLog (Join-Path (Split-Path $PSScriptRoot -Parent) 'logs') 'protect-secret'
Write-NumeraLog INFO AUTH 'User-scoped secret protection requested; value and path are not logged.'
try {
    if (-not $IsWindows -or -not [IO.Path]::IsPathFullyQualified($Path)) { throw 'An absolute Windows path is required for user-scoped DPAPI.' }
    if (Test-Path -LiteralPath $Path) { throw 'Secret file already exists; no overwrite is performed.' }
    if ($PSCmdlet.ShouldProcess('Explicit secret destination', 'Create user-scoped DPAPI encrypted secret')) {
        $secret = Read-Host 'Secret (hidden; never logged)' -AsSecureString
        try {
            $encrypted = $secret | ConvertFrom-SecureString
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path)) | Out-Null
            $file = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            try { $bytes = [Text.UTF8Encoding]::new($false).GetBytes($encrypted); $file.Write($bytes, 0, $bytes.Length) }
            finally { $file.Dispose() }
            $identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
            & icacls $Path /inheritance:r /grant:r "${identity}:(F)" 2>$null | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'Secret encryption succeeded but ACL restriction failed; restrict access before use.' }
            Write-NumeraLog INFO AUTH 'DPAPI secret created for the current Windows user; value not logged.'
        } finally { $secret.Dispose(); $encrypted = $null }
    } else { Write-NumeraLog INFO AUTH 'Secret protection not executed; no secret requested or file created.' }
} catch { Write-NumeraLog ERROR AUTH 'Secret protection failed. Verify destination, current-user DPAPI and access restrictions before use.'; exit 1 }
