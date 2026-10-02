#Requires -Version 7.0
[CmdletBinding(SupportsShouldProcess)]
param([Parameter(Mandatory)][string]$Path)
. (Join-Path $PSScriptRoot 'common.ps1')
Initialize-NumeraLog (Join-Path (Split-Path $PSScriptRoot -Parent) 'logs') 'protect-secret'
if (-not $IsWindows -or -not [IO.Path]::IsPathFullyQualified($Path)) { throw 'An absolute Windows path is required for user-scoped DPAPI.' }
if (Test-Path -LiteralPath $Path) { throw 'Secret file already exists; no overwrite is performed.' }
if ($PSCmdlet.ShouldProcess($Path, 'Create user-scoped DPAPI encrypted secret')) {
    $secret = Read-Host 'Secret (hidden; never logged)' -AsSecureString
    $encrypted = $secret | ConvertFrom-SecureString
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path)) | Out-Null
    [IO.File]::WriteAllText($Path, $encrypted, [Text.UTF8Encoding]::new($false))
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    & icacls $Path /inheritance:r /grant:r "${identity}:(F)" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Secret encryption succeeded but ACL restriction failed; restrict access before use.' }
    Write-NumeraLog INFO AUTH 'DPAPI secret created for the current Windows user; value not logged.'
}
