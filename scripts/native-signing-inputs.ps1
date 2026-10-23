#Requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Target)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'common.ps1')
Initialize-NumeraLog (Join-Path $root '.ci-work/native/logs') 'native-signing-inputs'
$private = Join-Path $root '.ci-work/signing'
$pfxPath = Join-Path $private 'publisher.pfx'
$passwordPath = Join-Path $private 'password.txt'
$pfx = $null
$passwordBytes = $null
$owned = $false
try {
    if ($env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REPOSITORY -cne 'KiaroSama/Numera-Image-Gen-MCP' -or
        $env:GITHUB_REF -cne 'refs/heads/feature/native-rust-distribution' -or $env:GITHUB_EVENT_NAME -cne 'workflow_dispatch') {
        throw 'Unauthorized signing route.'
    }
    if ($Target -cnotmatch '^(x86_64|aarch64)-(pc-windows-msvc|unknown-linux-gnu|apple-darwin)$') { throw 'Invalid target.' }
    if (Test-Path -LiteralPath $private) { throw 'Signing staging already exists; reconcile before retry.' }
    if ([string]::IsNullOrEmpty($env:NUMERA_SIGNING_PFX_BASE64) -or $env:NUMERA_SIGNING_PASSWORD -cnotmatch '^[A-Za-z0-9+/=]{1,4095}$') {
        throw 'Missing or malformed selected signing inputs.'
    }
    $pfx = [Convert]::FromBase64String($env:NUMERA_SIGNING_PFX_BASE64)
    if ($pfx.Length -gt 32768) { throw 'Private identity exceeds protected input bound.' }
    $password = $env:NUMERA_SIGNING_PASSWORD
    $passwordBytes = [Text.UTF8Encoding]::new($false, $true).GetBytes($password)
    if ($IsWindows) {
        $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
        $security = [Security.AccessControl.DirectorySecurity]::new()
        $security.SetAccessRuleProtection($true, $false)
        $security.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,
            [Security.AccessControl.FileSystemRights]::FullControl,
            ([Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit),
            [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow))
        [IO.FileSystemAclExtensions]::Create([IO.DirectoryInfo]::new($private), $security)
    } else {
        [IO.Directory]::CreateDirectory($private, [IO.UnixFileMode]448) | Out-Null
    }
    $owned = $true
    foreach ($item in @(@($pfxPath, $pfx), @($passwordPath, $passwordBytes))) {
        $options = [IO.FileStreamOptions]::new()
        $options.Mode = [IO.FileMode]::CreateNew
        $options.Access = [IO.FileAccess]::Write
        $options.Share = [IO.FileShare]::None
        if (-not $IsWindows) { $options.UnixCreateMode = [IO.UnixFileMode]384 }
        $file = [IO.FileStream]::new([string]$item[0], $options)
        try { $file.Write([byte[]]$item[1]); $file.Flush($true) } finally { $file.Dispose() }
    }
    [Environment]::SetEnvironmentVariable('NUMERA_SIGNING_PFX_BASE64', $null)
    [Environment]::SetEnvironmentVariable('NUMERA_SIGNING_PASSWORD', $null)
    $env:NUMERA_SIGNING_AUTHORIZED = 'true'
    $env:NUMERA_SIGNING_PFX = $pfxPath
    $env:NUMERA_SIGNING_PASSWORD_FILE = $passwordPath
    Write-NumeraLog INFO SIGNING 'Selected inputs staged privately; verifying certificate pin before native import and signing.'
    & python -I -B (Join-Path $PSScriptRoot 'native-signing.py') sign --target $Target --directory (Join-Path $root ('.ci-work/prepared/' + $Target))
    if ($LASTEXITCODE -ne 0) { throw 'Native signature verification failed.' }
    Write-NumeraLog INFO SIGNING 'Native signatures and approved manifest verified; disposing owned private inputs.'
} catch {
    Write-NumeraLog ERROR SIGNING 'Protected signing failed; private values and process output are not reported.'
    exit 1
} finally {
    foreach ($key in @('NUMERA_SIGNING_PFX_BASE64','NUMERA_SIGNING_PASSWORD','NUMERA_SIGNING_AUTHORIZED','NUMERA_SIGNING_PFX','NUMERA_SIGNING_PASSWORD_FILE')) {
        [Environment]::SetEnvironmentVariable($key, $null)
    }
    if ($pfx) { [Array]::Clear($pfx) }
    if ($passwordBytes) { [Array]::Clear($passwordBytes) }
    $password = $null
    if ($owned) {
        foreach ($path in @($pfxPath, $passwordPath)) { if ([IO.File]::Exists($path)) { [IO.File]::Delete($path) } }
        [IO.Directory]::Delete($private, $false)
        if (Test-Path -LiteralPath $private) { throw 'Owned signing input cleanup failed.' }
    }
}
