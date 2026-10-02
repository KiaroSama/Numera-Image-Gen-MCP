#Requires -Version 7.0
[CmdletBinding(SupportsShouldProcess)]
param([string]$ProjectRoot = (Split-Path $PSScriptRoot -Parent), [ValidateSet('Install','Update','Diagnose','Register','Remove')][string]$Action = 'Install', [ValidateSet('ClaudeProject','ClaudeDesktop','Codex')][string]$Client = 'ClaudeProject', [string]$ClientConfig, [string]$NumeraConfig)
. (Join-Path $PSScriptRoot 'common.ps1')
$ProjectRoot = [IO.Path]::GetFullPath($ProjectRoot)
Initialize-NumeraLog (Join-Path $ProjectRoot 'logs') 'setup'
Write-NumeraLog INFO SETUP "Action $Action requested."
try {
    $node = (Get-Command node -ErrorAction Stop).Source
    $npm = (Get-Command npm -ErrorAction Stop).Source
    $version = & $node --version
    if ($version -notmatch '^v24\.') { throw 'Node.js 24 is required. Install a supported runtime explicitly; setup does not relocate it.' }
    if ($Action -in @('Install','Update')) {
        if ($PSCmdlet.ShouldProcess($ProjectRoot, 'Install locked dependencies and build')) {
            Push-Location $ProjectRoot
            try { Invoke-NumeraCommand $npm @('ci','--no-audit','--no-fund'); Invoke-NumeraCommand $npm @('run','build'); Invoke-NumeraCommand $npm @('run','smoke') } finally { Pop-Location }
        }
    } elseif ($Action -eq 'Diagnose') {
        Push-Location $ProjectRoot
        try { Invoke-NumeraCommand $npm @('run','smoke') } finally { Pop-Location }
    } else {
        if (-not $ClientConfig -or -not [IO.Path]::IsPathFullyQualified($ClientConfig)) { throw 'Pass an absolute -ClientConfig for explicit entry-scoped registration/removal.' }
        if ($Action -eq 'Register' -and (-not $NumeraConfig -or -not [IO.Path]::IsPathFullyQualified($NumeraConfig))) { throw 'Pass an absolute -NumeraConfig; no credentials belong in host configuration.' }
        if ($Action -eq 'Register') { Push-Location $ProjectRoot; try { Invoke-NumeraCommand $npm @('run','smoke') } finally { Pop-Location } }
        if ($PSCmdlet.ShouldProcess($ClientConfig, "$Action only numera-image-gen entry")) {
            Invoke-NumeraCommand $node @((Join-Path $PSScriptRoot 'register.mjs'), $Client, $Action, $ClientConfig, $node, (Join-Path $ProjectRoot 'dist/index.js'), $NumeraConfig)
        }
    }
    Write-NumeraLog INFO SETUP 'Requested action completed.'
} catch { Write-NumeraLog ERROR SETUP $_.Exception.Message; exit 1 }
