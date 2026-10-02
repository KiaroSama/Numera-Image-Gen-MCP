Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:NumeraLog = $null
$script:NumeraRun = [Guid]::NewGuid().ToString('N')
function Write-NumeraLog {
    param([ValidateSet('INFO','WARNING','ERROR','DEBUG')][string]$Level, [string]$Component, [string]$Message)
    $entry = @{ timestamp = [DateTime]::UtcNow.ToString('o'); level = $Level; component = $Component; run_id = $script:NumeraRun; message = $Message } | ConvertTo-Json -Compress
    try { if ($script:NumeraLog) { [IO.File]::AppendAllText($script:NumeraLog, $entry + [Environment]::NewLine, [Text.UTF8Encoding]::new($false)) } }
    catch { $script:NumeraLog = $null; [Console]::Error.WriteLine('WARNING: File logging unavailable.') }
    [Console]::Error.WriteLine($entry)
}
function Initialize-NumeraLog {
    param([string]$Directory, [string]$Name)
    try {
        [IO.Directory]::CreateDirectory($Directory) | Out-Null
        $path = Join-Path $Directory ($Name + '_' + [DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss') + '_UTC-' + $script:NumeraRun + '.log')
        $file = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $file.Dispose()
        $script:NumeraLog = $path
    } catch { $script:NumeraLog = $null; [Console]::Error.WriteLine('WARNING: File logging unavailable.') }
}
function Assert-NumeraWriteAccess {
    param([string]$Directory)
    $path = Join-Path $Directory ('.numera-write-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    try { $file = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None); $file.Dispose() }
    finally { if ([IO.File]::Exists($path)) { [IO.File]::Delete($path) } }
}
function Invoke-NumeraCommand {
    param([string]$File, [string[]]$Arguments)
    Write-NumeraLog DEBUG PROCESS 'External command starting; arguments and output are not logged.'
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "External command failed with exit code $LASTEXITCODE." }
    Write-NumeraLog DEBUG PROCESS 'External command completed successfully.'
}
