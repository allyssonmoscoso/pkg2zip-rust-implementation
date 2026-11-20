[CmdletBinding()]
<#
.SYNOPSIS
    Genera el corpus sintético de PKG para las pruebas de paridad.

.EXAMPLE
    pwsh ./scripts/gen-corpus.ps1
#>
param(
    [string]$OutDir = 'target/corpus'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    & cargo run -q -p pkg2zip-testkit -- gen $OutDir
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
