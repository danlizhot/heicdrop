<#
.SYNOPSIS
  Construit le paquet MSIX de HeicDrop (pour le Microsoft Store ou l'installation locale).

.EXAMPLE
  # Les valeurs par défaut sont celles de l'Espace partenaires
  # (Gestion du produit > Identité du produit, ID Store 9NZTB7VCGFGH).
  .\packaging\build-msix.ps1 -Version 1.0.0.0

  Prérequis : `cargo build --release` déjà exécuté, DLL libheif disponibles
  (vcpkg, triplet x64-windows), SDK Windows 10/11 installé (makeappx, makepri).
#>
param(
    [string]$Version = "1.0.0.0",
    [string]$IdentityName = "DanielLizotte.HeicDrop",
    [string]$Publisher = "CN=EDE12BA8-4C62-4B6C-BCA0-C4A53527144E",
    [string]$PublisherDisplayName = "Daniel Lizotte",
    [string]$VcpkgBin = "$env:VCPKG_INSTALLATION_ROOT\installed\x64-windows\bin",
    [string]$OutDir = "dist"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if ($Version -notmatch '^\d+\.\d+\.\d+\.0$') {
    throw "La version MSIX doit être de la forme X.Y.Z.0 (le Store exige une révision à 0)."
}

# Outils du SDK Windows (dernière version installée)
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Directory |
       Where-Object { $_.Name -match '^10\.' } | Sort-Object Name -Descending | Select-Object -First 1
$makeappx = Join-Path $sdk.FullName "x64\makeappx.exe"
$makepri  = Join-Path $sdk.FullName "x64\makepri.exe"

# Dossier de préparation
$stage = Join-Path $OutDir "msix-stage"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path "$stage\HeicDrop" | Out-Null
Copy-Item "target\release\heicdrop.exe" "$stage\HeicDrop\"
Copy-Item "$VcpkgBin\*.dll" "$stage\HeicDrop\"
Copy-Item "packaging\msix\Assets" "$stage\Assets" -Recurse
Copy-Item "LICENSE", "THIRD_PARTY_NOTICES.md" "$stage\HeicDrop\"

# Manifeste avec l'identité Store
(Get-Content "packaging\msix\AppxManifest.xml" -Raw -Encoding UTF8).
    Replace("__IDENTITY_NAME__", $IdentityName).
    Replace("__PUBLISHER_DISPLAY_NAME__", [System.Security.SecurityElement]::Escape($PublisherDisplayName)).
    Replace("__PUBLISHER__", [System.Security.SecurityElement]::Escape($Publisher)).
    Replace("__VERSION__", $Version) |
    Set-Content "$stage\AppxManifest.xml" -Encoding UTF8

# Index des ressources (choix automatique des icônes selon l'échelle d'affichage)
$priconfig = Join-Path $OutDir "priconfig.xml"
& $makepri createconfig /cf $priconfig /dq fr-FR /pv 10.0.0 /o
if ($LASTEXITCODE -ne 0) { throw "makepri createconfig a échoué" }
& $makepri new /pr $stage /cf $priconfig /of "$stage\resources.pri" /o
if ($LASTEXITCODE -ne 0) { throw "makepri a échoué" }

$msix = Join-Path $OutDir "HeicDrop_${Version}_x64.msix"
& $makeappx pack /d $stage /p $msix /o
if ($LASTEXITCODE -ne 0) { throw "makeappx a échoué" }
Write-Host "Paquet créé : $msix"
