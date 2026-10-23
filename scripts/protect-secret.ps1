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
            $security = [Security.AccessControl.FileSecurity]::new()
            $security.SetAccessRuleProtection($true, $false)
            $security.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.WindowsIdentity]::GetCurrent().User, [Security.AccessControl.FileSystemRights]::FullControl, [Security.AccessControl.AccessControlType]::Allow))
            # Apply the complete DACL at creation; disabling inheritance alone retains default-token grants.
            $file = [IO.FileSystemAclExtensions]::Create([IO.FileInfo]::new($Path), [IO.FileMode]::CreateNew, [Security.AccessControl.FileSystemRights]::Write, [IO.FileShare]::None, 4096, [IO.FileOptions]::None, $security)
            try { $bytes = [Text.UTF8Encoding]::new($false).GetBytes($encrypted); $file.Write($bytes, 0, $bytes.Length) }
            finally { $file.Dispose() }
            Write-NumeraLog INFO AUTH 'DPAPI secret created for the current Windows user; value not logged.'
        } finally { $secret.Dispose(); $encrypted = $null }
    } else { Write-NumeraLog INFO AUTH 'Secret protection not executed; no secret requested or file created.' }
} catch { Write-NumeraLog ERROR AUTH 'Secret protection failed. Verify destination, current-user DPAPI and access restrictions before use.'; exit 1 }
