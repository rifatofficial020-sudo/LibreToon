$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Set-Location -LiteralPath $repo
$env:CARGO_TARGET_DIR = Join-Path $repo 'target'
function Get-SourceStamp {
    $files = @(Get-ChildItem -LiteralPath (Join-Path $repo 'src') -Recurse -File)
    $files += Get-Item -LiteralPath (Join-Path $repo 'Cargo.toml')
    return (($files | Sort-Object FullName | ForEach-Object { $_.FullName + ':' + $_.LastWriteTimeUtc.Ticks }) -join '|')
}
# Let the editor flush its pending recovery snapshot before a restart.
function Close-LibreToon {
    param([System.Diagnostics.Process]$Process)
    if ($null -eq $Process -or $Process.HasExited) { return }
    $Process.Refresh()
    if ($Process.CloseMainWindow()) {
        $Process.WaitForExit()
    } else {
        # The application has not created its window yet.
        Stop-Process -Id $Process.Id -ErrorAction SilentlyContinue
        $Process.WaitForExit()
    }
}
$appProcess = $null
$stamp = ''
try {
    while ($true) {
        $next = Get-SourceStamp
        if ($next -ne $stamp) {
            $stamp = $next
            if ($null -ne $appProcess -and -not $appProcess.HasExited) {
                Close-LibreToon -Process $appProcess
                $appProcess = $null
            }
            Write-Host 'Compilando LibreToon...' -ForegroundColor Cyan
            & cargo build --locked
            if ($LASTEXITCODE -eq 0) {
                $appProcess = Start-Process -FilePath (Join-Path $repo 'target\debug\libretoon.exe') -PassThru
                Write-Host 'LibreToon abierto. Guarda cambios en src para recompilar. Ctrl+C para terminar.' -ForegroundColor Green
            } else { Write-Host 'Error de compilacion. Corrige el codigo; se reintentara al guardar.' -ForegroundColor Yellow }
        }
        if ($null -ne $appProcess -and $appProcess.HasExited -and $next -eq (Get-SourceStamp)) { break }
        Start-Sleep -Milliseconds 600
    }
} finally {
    if ($null -ne $appProcess -and -not $appProcess.HasExited) { Close-LibreToon -Process $appProcess }
}


