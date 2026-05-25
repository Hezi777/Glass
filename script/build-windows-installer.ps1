#Requires -Version 5.1
param(
    [Parameter(Mandatory = $true)]
    [string]$ReleaseDir,   # e.g. target\x86_64-pc-windows-msvc\release
    [Parameter(Mandatory = $true)]
    [string]$Version,      # e.g. v0.0.36 or 0.0.36
    [Parameter(Mandatory = $true)]
    [string]$OutPath       # full path for output .msi
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$version = $Version -replace '^v', ''
# MSI requires a 4-part version number
$version4 = "$version.0"

$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseDir = (Resolve-Path $ReleaseDir).Path
$iconPath = Join-Path $repoRoot "crates\zed\resources\windows\app-icon.ico"
$iconPath = (Resolve-Path $iconPath).Path

# Locate WiX 3 tools (pre-installed on windows-2022 runners)
$wixBin = Get-ChildItem "C:\Program Files (x86)" -Directory -Filter "WiX Toolset v3*" -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending |
    Select-Object -First 1 |
    ForEach-Object { Join-Path $_.FullName "bin" }

if (-not $wixBin -or -not (Test-Path (Join-Path $wixBin "candle.exe"))) {
    Write-Host "WiX 3 not found, installing..."
    choco install wixtoolset -y --no-progress
    $wixBin = Get-ChildItem "C:\Program Files (x86)" -Directory -Filter "WiX Toolset v3*" |
        Sort-Object Name -Descending |
        Select-Object -First 1 |
        ForEach-Object { Join-Path $_.FullName "bin" }
}

$heat   = Join-Path $wixBin "heat.exe"
$candle = Join-Path $wixBin "candle.exe"
$light  = Join-Path $wixBin "light.exe"
Write-Host "WiX tools: $wixBin"

# Build a clean staging directory so heat only sees installer files
$stageDir = Join-Path $env:TEMP "glass-stage-$([System.Guid]::NewGuid().ToString('N').Substring(0,8))"
New-Item -ItemType Directory -Path $stageDir -Force | Out-Null
New-Item -ItemType Directory -Path "$stageDir\locales" -Force | Out-Null

$knownExes = @("zed.exe", "cli.exe", "glass_helper.exe", "conpty.dll", "OpenConsole.exe")
foreach ($name in $knownExes) {
    $src = Join-Path $releaseDir $name
    if (Test-Path $src) {
        Copy-Item $src (Join-Path $stageDir $name) -Force
    }
}

# CEF runtime files
Get-ChildItem $releaseDir -File |
    Where-Object { $_.Extension -in @('.dll', '.pak', '.dat', '.bin') -and $_.Name -ne 'archive.json' } |
    ForEach-Object { Copy-Item $_.FullName (Join-Path $stageDir $_.Name) -Force }

# Locales
if (Test-Path "$releaseDir\locales") {
    Get-ChildItem "$releaseDir\locales" -File |
        ForEach-Object { Copy-Item $_.FullName (Join-Path $stageDir "locales\$($_.Name)") -Force }
}

Write-Host "Staged $(( Get-ChildItem $stageDir -Recurse -File ).Count) files"

# Work directory for WiX intermediates
$workDir = Join-Path $env:TEMP "glass-wix-$([System.Guid]::NewGuid().ToString('N').Substring(0,8))"
New-Item -ItemType Directory -Path $workDir -Force | Out-Null

# Minimal license RTF (required by WixUI_InstallDir)
$licenseRtf = Join-Path $workDir "license.rtf"
@'
{\rtf1\ansi\deff0{\fonttbl{\f0\fnil\fcharset0 Arial;}}
\f0\fs18 Glass is open-source software licensed under the terms of the upstream Zed repository.
Visit https://github.com/Glass-HQ/Glass for full license details.\par
}
'@ | Set-Content -Path $licenseRtf -Encoding ASCII

# Fixed GUIDs — UpgradeCode must never change across releases
$upgradeCode  = "9F7E4C2A-8B3D-4F5A-A1C0-6D2E8F4B7C3A"
$shortcutGuid = "3B8D1F5A-7C2E-4A9B-B3D6-1E5F8A2C4B7D"

# Product.wxs
$productWxs = Join-Path $workDir "Product.wxs"
@"
<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Product Id="*"
           Name="Glass"
           Language="1033"
           Version="$version4"
           Manufacturer="Glass HQ"
           UpgradeCode="$upgradeCode">

    <Package InstallerVersion="500"
             Compressed="yes"
             InstallScope="perMachine"
             Platform="x64"
             Description="Glass $version"
             Manufacturer="Glass HQ" />

    <MajorUpgrade DowngradeErrorMessage="A newer version of Glass is already installed." />
    <MediaTemplate EmbedCab="yes" />

    <Icon Id="GlassIcon.ico" SourceFile="$iconPath" />
    <Property Id="ARPPRODUCTICON"  Value="GlassIcon.ico" />
    <Property Id="ARPHELPLINK"     Value="https://github.com/Glass-HQ/Glass" />
    <Property Id="ARPURLINFOABOUT" Value="https://github.com/Glass-HQ/Glass" />

    <UIRef Id="WixUI_InstallDir" />
    <Property Id="WIXUI_INSTALLDIR" Value="INSTALLFOLDER" />
    <WixVariable Id="WixUILicenseRtf" Value="$licenseRtf" />

    <Directory Id="TARGETDIR" Name="SourceDir">
      <Directory Id="ProgramFiles64Folder">
        <Directory Id="INSTALLFOLDER" Name="Glass" />
      </Directory>
      <Directory Id="ProgramMenuFolder">
        <Directory Id="GlassMenuDir" Name="Glass" />
      </Directory>
      <Directory Id="DesktopFolder" />
    </Directory>

    <DirectoryRef Id="GlassMenuDir">
      <Component Id="ShortcutComp" Guid="$shortcutGuid" Win64="yes">
        <Shortcut Id="StartMenuShortcut"
                  Name="Glass"
                  Description="Glass Browser"
                  Target="[INSTALLFOLDER]zed.exe"
                  WorkingDirectory="INSTALLFOLDER"
                  Icon="GlassIcon.ico" />
        <Shortcut Id="DesktopShortcut"
                  Name="Glass"
                  Description="Glass Browser"
                  Target="[INSTALLFOLDER]zed.exe"
                  WorkingDirectory="INSTALLFOLDER"
                  Icon="GlassIcon.ico"
                  Directory="DesktopFolder" />
        <RemoveFolder Id="RemoveGlassMenuDir" Directory="GlassMenuDir" On="uninstall" />
        <RegistryValue Root="HKCU"
                       Key="Software\Glass"
                       Name="installed"
                       Type="integer"
                       Value="1"
                       KeyPath="yes" />
      </Component>
    </DirectoryRef>

    <Feature Id="MainFeature" Title="Glass" Level="1">
      <ComponentGroupRef Id="AppFiles" />
      <ComponentRef Id="ShortcutComp" />
    </Feature>

  </Product>
</Wix>
"@ | Set-Content -Path $productWxs -Encoding UTF8

# Harvest all staged files into a ComponentGroup
$harvestWxs = Join-Path $workDir "AppFiles.wxs"
Write-Host "Harvesting staged files..."
& $heat dir "$stageDir" `
    -cg AppFiles `
    -dr INSTALLFOLDER `
    -scom -sreg -srd `
    -var var.SourceDir `
    -gg `
    -out "$harvestWxs"
if ($LASTEXITCODE -ne 0) { throw "heat.exe failed ($LASTEXITCODE)" }

# Compile
Write-Host "Compiling..."
& $candle -arch x64 `
    "-dSourceDir=$stageDir" `
    "$productWxs" "$harvestWxs" `
    -out "$workDir\"
if ($LASTEXITCODE -ne 0) { throw "candle.exe failed ($LASTEXITCODE)" }

# Link
Write-Host "Linking MSI..."
New-Item -ItemType Directory -Force -Path (Split-Path $OutPath) | Out-Null
& $light `
    "$workDir\Product.wixobj" `
    "$workDir\AppFiles.wixobj" `
    -ext WixUIExtension `
    -out "$OutPath"
if ($LASTEXITCODE -ne 0) { throw "light.exe failed ($LASTEXITCODE)" }

Write-Host "MSI ready: $OutPath"
