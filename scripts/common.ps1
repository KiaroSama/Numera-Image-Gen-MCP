Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Write-NumeraLog {
    param([ValidateSet('INFO','WARNING','ERROR','DEBUG')][string]$Level, [string]$Component, [string]$Message)
    $entry = @{ timestamp = [DateTime]::UtcNow.ToString('o'); level = $Level; component = $Component; message = $Message } | ConvertTo-Json -Compress
    if ($script:NumeraLog) { [IO.File]::AppendAllText($script:NumeraLog, $entry + [Environment]::NewLine, [Text.UTF8Encoding]::new($false)) }
    [Console]::Error.WriteLine($entry)
}
function Initialize-NumeraLog {
    param([string]$Directory, [string]$Name)
    try { [IO.Directory]::CreateDirectory($Directory) | Out-Null; $script:NumeraLog = Join-Path $Directory ($Name + '_' + [DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss') + '_UTC-' + [Guid]::NewGuid().ToString('N') + '.log') }
    catch { $script:NumeraLog = $null; [Console]::Error.WriteLine('WARNING: File logging unavailable.') }
}
function Invoke-NumeraCommand {
    param([string]$File, [string[]]$Arguments)
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "External command failed with exit code $LASTEXITCODE." }
}
