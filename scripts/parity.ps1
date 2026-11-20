[CmdletBinding()]
<#
.SYNOPSIS
    Ejecuta el arnés de paridad C vs Rust sobre el corpus sintético.

.EXAMPLE
    pwsh ./scripts/parity.ps1
    pwsh ./scripts/parity.ps1 -Rust target/release/pkg2zip.exe -Report target/parity.json
#>
param(
    [string]$Corpus = 'target/corpus',
    [string]$Legacy = 'target/legacy/pkg2zip.exe',
    [string]$Rust = 'target/debug/pkg2zip.exe',
    [string]$Report = 'target/parity.json'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    & cargo run -q -p pkg2zip-testkit -- parity `
        --corpus $Corpus `
        --legacy $Legacy `
        --rust $Rust `
        --report $Report
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
