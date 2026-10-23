#Requires -Version 7.0
[CmdletBinding(SupportsShouldProcess)]
param([switch]$Inspect)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'common.ps1')
Initialize-NumeraLog (Join-Path $root 'logs') 'native-signing-identity'
$private = Join-Path $root '.runtime/signing'
$public = Join-Path $root 'docs/signing'
$registry = Join-Path $root 'secrets.md'
$utf8 = [Text.UTF8Encoding]::new($false, $true)
function New-PrivateFile([string]$Path, [byte[]]$Bytes) {
    $security = [Security.AccessControl.FileSecurity]::new()
    $security.SetAccessRuleProtection($true, $false)
    $security.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
        [Security.Principal.WindowsIdentity]::GetCurrent().User,
        [Security.AccessControl.FileSystemRights]::FullControl,
        [Security.AccessControl.AccessControlType]::Allow))
    $file = [IO.FileSystemAclExtensions]::Create([IO.FileInfo]::new($Path), [IO.FileMode]::CreateNew,
        [Security.AccessControl.FileSystemRights]::Write, [IO.FileShare]::None, 4096,
        [IO.FileOptions]::None, $security)
    try { $file.Write($Bytes); $file.Flush($true) } finally { $file.Dispose() }
}
try {
    if (-not $IsWindows) { throw 'Identity provisioning requires the approved Windows custody host.' }
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
    if (-not [IO.File]::Exists($registry)) { throw 'The local secret registry must already exist.' }
    $registryInfo = [IO.FileInfo]::new($registry)
    if (($registryInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $registryInfo.Length -gt 1048576) {
        throw 'The local registry must be a bounded regular file.'
    }
    $acl = Get-Acl -LiteralPath $registry
    $foreign = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]) |
        Where-Object { $_.AccessControlType -eq 'Allow' -and $_.IdentityReference -ne $sid })
    if ($Inspect) {
        [Console]::WriteLine((@{ registry_present = $true; registry_owner_only = ($foreign.Count -eq 0);
            private_identity_exists = (Test-Path -LiteralPath $private);
            public_identity_exists = (Test-Path -LiteralPath (Join-Path $public 'publisher-key.der')) } | ConvertTo-Json -Compress))
        return
    }
    if ((Test-Path -LiteralPath $private) -or (Test-Path -LiteralPath (Join-Path $public 'publisher-key.der'))) {
        throw 'Signing identity already exists; no implicit overwrite or rotation is permitted.'
    }
    if ($acl.GetOwner([Security.Principal.SecurityIdentifier]) -ne $sid) { throw 'Local registry is not owned by the approved user.' }
    if (-not $PSCmdlet.ShouldProcess('Approved project identity', 'Protect local key custody and create KiaroSama self-signed signing material')) { return }
    $directorySecurity = [Security.AccessControl.DirectorySecurity]::new()
    $directorySecurity.SetAccessRuleProtection($true, $false)
    $directorySecurity.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,
        [Security.AccessControl.FileSystemRights]::FullControl,
        ([Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit),
        [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow))
    $runtimePath = Split-Path $private -Parent
    if ((Test-Path -LiteralPath $runtimePath) -and ((Get-Item -LiteralPath $runtimePath).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Private custody parent must not be a reparse point.'
    }
    [IO.Directory]::CreateDirectory($runtimePath) | Out-Null
    [IO.FileSystemAclExtensions]::Create([IO.DirectoryInfo]::new($private), $directorySecurity)
    New-PrivateFile (Join-Path $private 'registry-before.sddl') ($utf8.GetBytes($acl.Sddl))
    if ($foreign.Count -ne 0) {
        $restricted = [Security.AccessControl.FileSecurity]::new()
        $restricted.SetAccessRuleProtection($true, $false)
        $restricted.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,
            [Security.AccessControl.FileSystemRights]::FullControl, [Security.AccessControl.AccessControlType]::Allow))
        Set-Acl -LiteralPath $registry -AclObject $restricted
        $after = Get-Acl -LiteralPath $registry
        $foreignAfter = @($after.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]) |
            Where-Object { $_.AccessControlType -eq 'Allow' -and $_.IdentityReference -ne $sid })
        if ($foreignAfter.Count -ne 0 -or -not $after.AreAccessRulesProtected) { throw 'Private registry access could not be verified.' }
    }
    Write-NumeraLog INFO SIGNING 'Private custody verified; creating the approved in-memory signing identity.'
    $rsa = [Security.Cryptography.RSA]::Create(3072)
    $cert = $null
    $pfx = $null
    $password = $null
    try {
        $request = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=KiaroSama', $rsa,
            [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
        $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new(
            [Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature, $true))
        $oids = [Security.Cryptography.OidCollection]::new()
        [void]$oids.Add([Security.Cryptography.Oid]::new('1.3.6.1.5.5.7.3.3'))
        $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]::new($oids, $true))
        $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($false, $false, 0, $true))
        $now = [DateTimeOffset]::UtcNow
        $cert = $request.CreateSelfSigned($now.AddMinutes(-10), $now.AddYears(1))
        $password = [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(48))
        $parameters = [Security.Cryptography.PbeParameters]::new([Security.Cryptography.PbeEncryptionAlgorithm]::Aes256Cbc,
            [Security.Cryptography.HashAlgorithmName]::SHA256, 200000)
        $pfx = $cert.ExportPkcs12($parameters, $password)
        $publicKey = $rsa.ExportRSAPublicKey()
        $certificate = $cert.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert)
        $keyHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($publicKey)).ToLowerInvariant()
        $certHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($certificate)).ToLowerInvariant()
        New-PrivateFile (Join-Path $private 'publisher.pfx') $pfx
        New-PrivateFile (Join-Path $private 'publisher-password.txt') ($utf8.GetBytes($password))
        # Read/append under an exclusive handle; preserve the existing registry bytes exactly.
        $file = [IO.File]::Open($registry, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        try {
            $bytes = [byte[]]::new($file.Length)
            $file.ReadExactly($bytes)
            $text = $utf8.GetString($bytes)
            if ($text.Contains('NUMERA_SIGNING_PFX_PASSWORD')) { throw 'Existing signing registry entry requires reconciliation, not overwrite.' }
            $entry = "`n## Native artifact signing`n- Secret: NUMERA_SIGNING_PFX_PASSWORD`n- Purpose: approved KiaroSama self-signed native artifact signing only.`n- Service: protected final hosted signing; not configured remotely by this command.`n- Source: owner-authorized project identity creation2026-10-17.`n- Updated UTC: $($now.ToString('o'))`n- Private binary custody: .runtime/signing/publisher.pfx (encrypted; preserve for recovery).`n- Password value: $password`n- Recovery: use this same PFX/password; never rotate on a failed build. No OS trust installation.`n"
            $file.Position = $file.Length
            $file.Write($utf8.GetBytes($entry))
            $file.Flush($true)
        } finally { $file.Dispose() }
        [IO.Directory]::CreateDirectory($public) | Out-Null
        foreach ($item in @(@('publisher-key.der', $publicKey), @('publisher-cert.der', $certificate))) {
            $file = [IO.File]::Open((Join-Path $public $item[0]), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            try { $file.Write([byte[]]$item[1]); $file.Flush($true) } finally { $file.Dispose() }
        }
        $metadata = @{ publisher = 'KiaroSama'; product = 'Numera Image-Gen MCP'; algorithm = 'rsa-pkcs1-sha256';
            public_key_sha256 = $keyHash; certificate_sha256 = $certHash; certificate_thumbprint = $cert.Thumbprint;
            not_before = $cert.NotBefore.ToUniversalTime().ToString('o'); not_after = $cert.NotAfter.ToUniversalTime().ToString('o');
            trust = 'self-signed; not public OS trust, Apple Developer ID or notarization'; timestamp = 'none; verification requires current certificate validity' }
        $file = [IO.File]::Open((Join-Path $public 'publisher.json'), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $file.Write($utf8.GetBytes(($metadata | ConvertTo-Json) + "`n")); $file.Flush($true) } finally { $file.Dispose() }
        Write-NumeraLog INFO SIGNING 'Approved self-signed public identity created; private values remain in local custody. No trust store or remote secrets changed.'
        [Console]::WriteLine((@{ publisher = 'KiaroSama'; public_key_sha256 = $keyHash; private_custody_created = $true;
            registry_updated = $true; os_trust_changed = $false; remote_secret_changed = $false } | ConvertTo-Json -Compress))
    } finally {
        if ($pfx) { [Array]::Clear($pfx) }
        $password = $null
        if ($cert) { $cert.Dispose() }
        $rsa.Dispose()
    }
} catch {
    Write-NumeraLog ERROR SIGNING 'Identity provisioning failed; preserve any partial custody and reconcile before retry. Private values and paths are not logged.'
    exit 1
}
