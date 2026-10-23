#Requires -Version 7.0
[CmdletBinding()]
param([switch]$WriteFixtures)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'common.ps1')
Initialize-NumeraLog (Join-Path $root 'logs') 'native-signing-check'
$rsa = $null
$cert = $null
$wrong = $null
try {
    $private = Join-Path $root '.runtime/signing'
    $public = Join-Path $root 'docs/signing'
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    Write-NumeraLog INFO SIGNING 'Opening existing encrypted identity; no key rotation or trust mutation.'
    $password = [IO.File]::ReadAllText((Join-Path $private 'publisher-password.txt'), $utf8)
    $cert = [Security.Cryptography.X509Certificates.X509CertificateLoader]::LoadPkcs12FromFile(
        (Join-Path $private 'publisher.pfx'), $password,
        [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
    $rsa = [Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($cert)
    $key = [IO.File]::ReadAllBytes((Join-Path $public 'publisher-key.der'))
    $metadata = [IO.File]::ReadAllText((Join-Path $public 'publisher.json'), $utf8) | ConvertFrom-Json
    if ($metadata.publisher -ne 'KiaroSama' -or $cert.Subject -ne 'CN=KiaroSama' -or
        [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($key)).ToLowerInvariant() -ne $metadata.public_key_sha256 -or
        [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($cert.RawData)).ToLowerInvariant() -ne $metadata.certificate_sha256 -or
        -not [Security.Cryptography.CryptographicOperations]::FixedTimeEquals($key, $rsa.ExportRSAPublicKey())) {
        throw 'Approved identity binding failed.'
    }
    $message = $utf8.GetBytes('Numera approved identity 日本語 fixture')
    $signature = $rsa.SignData($message, [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    if (-not $rsa.VerifyData($message, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Positive signature failed.' }
    $changed = [byte[]]$message.Clone()
    $changed[0] = $changed[0] -bxor 1
    if ($rsa.VerifyData($changed, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Tamper was accepted.' }
    Write-NumeraLog INFO SIGNING 'Approved public-key binding and tamper rejection verified; checking unrelated signer.'
    $wrong = [Security.Cryptography.RSA]::Create(3072)
    if ($wrong.VerifyData($message, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Wrong signer was accepted.' }
    if ($WriteFixtures) {
        foreach ($variant in @('approved', 'update')) {
        $fixtures = Join-Path $root ('tests/fixtures/native-signing/' + $variant)
        [IO.Directory]::CreateDirectory($fixtures) | Out-Null
        foreach ($target in @('x86_64-pc-windows-msvc','aarch64-pc-windows-msvc','x86_64-unknown-linux-gnu',
            'aarch64-unknown-linux-gnu','x86_64-apple-darwin','aarch64-apple-darwin')) {
            $binary = if ($target.Contains('windows')) { 'bin/numera-image-gen-mcp.exe' } else { 'bin/numera-image-gen-mcp' }
            $binaryBytes = $utf8.GetBytes($(if ($variant -eq 'update') {
                'updated synthetic install fixture; never executed'
            } else { 'synthetic install fixture; never executed' }))
            $logoBytes = [IO.File]::ReadAllBytes((Join-Path $root 'docs/assets/logo.png'))
            $hash = { param([byte[]]$Bytes) [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($Bytes)).ToLowerInvariant() }
            $version = if ($variant -eq 'update') { '0.2.0' } else { '0.1.0' }
            $manifest = [ordered]@{ version=$version; target=$target; source_sha='fc7da25c3b470199949b1b4c67127d77afa0c34a';
                publisher='KiaroSama'; binary_sha256=(& $hash $binaryBytes); files=@(
                    [ordered]@{path=$binary; sha256=(& $hash $binaryBytes); bytes=$binaryBytes.Length},
                    [ordered]@{path='docs/assets/logo.png'; sha256=(& $hash $logoBytes); bytes=$logoBytes.Length}) }
            $payload = $utf8.GetBytes(($manifest | ConvertTo-Json -Depth 8 -Compress))
            $signed = $rsa.SignData($payload, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
            $wrongSigned = $wrong.SignData($payload, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
            foreach ($item in @(@(($target+'.json'),$payload), @(($target+'.sig'),$signed), @(($target+'.wrong.sig'),$wrongSigned))) {
                $file = [IO.File]::Open((Join-Path $fixtures $item[0]), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                try { $file.Write([byte[]]$item[1]); $file.Flush($true) } finally { $file.Dispose() }
            }
        }
        }
    }
    $registry = [IO.File]::ReadAllText((Join-Path $root 'secrets.md'), $utf8)
    if (-not $registry.Contains('- Password value: ' + $password)) { throw 'Local recovery custody is incomplete.' }
    Write-NumeraLog INFO SIGNING 'Encrypted identity recovery, approved public binding, Unicode round-trip and tamper/wrong-signer rejection verified. No artifact signing or trust claim.'
    [Console]::WriteLine((@{ identity_recovered = $true; public_binding = $true; unicode_roundtrip = $true;
        tamper_rejected = $true; wrong_signer_rejected = $true; local_recovery_verified = $true;
        artifact_signed = $false; os_trust_changed = $false } | ConvertTo-Json -Compress))
} catch {
    Write-NumeraLog ERROR SIGNING 'Private identity verification failed; preserve custody. Secret values and exception data are not logged.'
    exit 1
} finally {
    $password = $null
    $registry = $null
    if ($rsa) { $rsa.Dispose() }
    if ($cert) { $cert.Dispose() }
    if ($wrong) { $wrong.Dispose() }
}
