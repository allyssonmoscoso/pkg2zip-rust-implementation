# Requires -Version 7.0
<#
.SYNOPSIS
    Compila la implementación C congelada de legacy/ usando MSVC (localizado con vswhere).

.DESCRIPTION
    Produce target/legacy/pkg2zip.exe. No requiere que cl.exe esté en el PATH: usa VsDevCmd.bat de
    la instalación de Visual Studio más reciente que tenga las C++ build tools.

.PARAMETER Force
    Recompila aunque el binario ya exista.

.PARAMETER OutDir
    Directorio de salida (por defecto target/legacy).
#>
[CmdletBinding()]
param(
    [string]$OutDir = '',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$legacy = Join-Path $root 'legacy'
if ($OutDir -eq '') { $OutDir = Join-Path $root 'target/legacy' }
$out = [System.IO.Path]::GetFullPath($OutDir)

if (-not (Test-Path $legacy)) { throw "No existe $legacy" }
New-Item -ItemType Directory -Force -Path $out | Out-Null

$exe = Join-Path $out 'pkg2zip.exe'
if ((Test-Path $exe) -and -not $Force) {
    Write-Host "[legacy] ya compilado: $exe (usa -Force para recompilar)"
    exit 0
}

$sources = Get-ChildItem -Path $legacy -Filter '*.c' -File | Sort-Object Name | ForEach-Object { $_.FullName }
if ($sources.Count -eq 0) { throw "No hay fuentes .c en $legacy" }

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) {
    throw 'vswhere.exe no encontrado. Instala Visual Studio Build Tools (C++) o usa scripts/build-legacy.sh con gcc.'
}

$vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsPath) { throw 'No hay ninguna instalación de Visual Studio con C++ build tools.' }

$devcmd = Join-Path $vsPath 'Common7\Tools\VsDevCmd.bat'
if (-not (Test-Path $devcmd)) { throw "VsDevCmd.bat no encontrado en $vsPath" }

$quoted = ($sources | ForEach-Object { '"' + $_ + '"' }) -join ' '
# Los .obj se generan en el directorio de salida (cd) para no ensuciar legacy/.
$cmd = 'call "{0}" -arch=x64 -host_arch=x64 >nul && cd /d "{1}" && cl.exe /nologo /O2 /DNDEBUG /Fepkg2zip.exe {2}' -f $devcmd, $out, $quoted

Write-Host "[legacy] Visual Studio: $vsPath"
Write-Host "[legacy] compilando $($sources.Count) archivos .c -> $exe"

& cmd.exe /d /c $cmd
if ($LASTEXITCODE -ne 0) { throw "cl.exe terminó con código $LASTEXITCODE" }
if (-not (Test-Path $exe)) { throw "cl.exe no produjo $exe" }

Write-Host "[legacy] OK: $exe"
