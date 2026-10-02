#Requires -Version 7.0
[CmdletBinding(SupportsShouldProcess)]
param([string]$ProjectRoot = (Split-Path $PSScriptRoot -Parent), [ValidateSet('Install','Update','Diagnose','Register','Remove')][string]$Action = 'Install', [ValidateSet('ClaudeProject','ClaudeDesktop','Codex')][string]$Client = 'ClaudeProject', [string]$ClientConfig, [string]$NumeraConfig)
. (Join-Path $PSScriptRoot 'common.ps1')
$ProjectRoot = [IO.Path]::GetFullPath($ProjectRoot)
Initialize-NumeraLog (Join-Path $ProjectRoot 'logs') 'setup'
Write-NumeraLog INFO SETUP "Action $Action requested."
try {
    if (-not [IO.Directory]::Exists($ProjectRoot)) { throw 'Project directory is unavailable.' }
    $node = (Get-Command node -ErrorAction Stop).Source
    $version = & $node --version
    if ($LASTEXITCODE -ne 0 -or $version -notmatch '^v24\.(\d+)\.(\d+)$' -or [int]$Matches[1] -lt 21) { throw 'Node.js >=24.21.0 <25 is required. Setup never installs or relocates a runtime.' }
    Write-NumeraLog DEBUG SETUP 'Supported Node.js runtime verified.'
    if ($Action -in @('Register','Remove')) {
        if (-not $ClientConfig -or -not [IO.Path]::IsPathFullyQualified($ClientConfig)) { throw 'Pass an absolute -ClientConfig for explicit entry-scoped registration/removal.' }
        if ($Action -eq 'Register' -and (-not $NumeraConfig -or -not [IO.Path]::IsPathFullyQualified($NumeraConfig))) { throw 'Pass an absolute -NumeraConfig; host configuration does not store credentials.' }
    }
    # One gate surrounds every executable mutation, including Diagnose/registration smoke checks.
    if ($PSCmdlet.ShouldProcess('Numera project and explicitly selected host entry', "$Action requested action")) {
        Assert-NumeraWriteAccess $ProjectRoot
        $bounded = Join-Path $PSScriptRoot 'bounded.mjs'
        Push-Location $ProjectRoot
        try {
            if ($Action -in @('Install','Update')) {
                $npmSource = (Get-Command npm -ErrorAction Stop).Source
                $npm = Join-Path (Split-Path $npmSource -Parent) 'node_modules/npm/bin/npm-cli.js'
                if (-not [IO.File]::Exists($npm)) { $npm = Join-Path (Split-Path $node -Parent) 'node_modules/npm/bin/npm-cli.js' }
                if (-not [IO.File]::Exists($npm)) { throw 'Installed npm CLI is unavailable; setup does not install it.' }
                Invoke-NumeraCommand $node @($bounded,'180000','45000',$npm,'ci','--no-audit','--no-fund')
                Invoke-NumeraCommand $node @($bounded,'60000','20000',$npm,'run','build')
            }
            if ($Action -in @('Install','Update','Diagnose','Register')) {
                Invoke-NumeraCommand $node @($bounded,'20000','10000',(Join-Path $PSScriptRoot 'smoke.mjs'))
            }
            if ($Action -in @('Register','Remove')) {
                Invoke-NumeraCommand $node @($bounded,'20000','10000',(Join-Path $PSScriptRoot 'register.mjs'),$Client,$Action,$ClientConfig,$node,(Join-Path $ProjectRoot 'dist/index.js'),$NumeraConfig)
            }
        } finally { Pop-Location }
        Write-NumeraLog INFO SETUP 'Requested action completed.'
    } else { Write-NumeraLog INFO SETUP 'Requested action not executed; no build, smoke, dependency or host changes.' }
} catch { Write-NumeraLog ERROR SETUP 'Setup failed. Verify runtime, project write access and explicit configuration; no credentials or private paths are logged.'; exit 1 }
