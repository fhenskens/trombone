param(
    [string]$Target = $(if ($env:WINDOWS_TARGET) { $env:WINDOWS_TARGET } else { "x86_64-pc-windows-msvc" }),
    [string]$Bin,
    [string]$Example,
    [ValidateSet("auto", "shared", "exclusive")]
    [string]$WasapiMode = $(if ($env:TROMBONE_WASAPI_MODE) { $env:TROMBONE_WASAPI_MODE } else { "" }),
    [switch]$Release,
    [switch]$NoBuild,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$AppArgs
)

$ErrorActionPreference = "Stop"
$defaultTarget = if ($env:WINDOWS_TARGET) { $env:WINDOWS_TARGET } else { "x86_64-pc-windows-msvc" }

function Show-Usage {
    @"
Usage:
  .\scripts\run-windows.ps1 [-Target <rust-target>] [-Bin <name> | -Example <name>] [-WasapiMode <auto|shared|exclusive>] [-Release] [-NoBuild] [-- <app-args>]

Examples:
  .\scripts\run-windows.ps1
  .\scripts\run-windows.ps1 -Example windows_capture -- --seconds 5
  .\scripts\run-windows.ps1 -Example windows_duplex -Release -- --seconds 5 --gain 1.0
  .\scripts\run-windows.ps1 -Example windows_bench -WasapiMode exclusive --% --mode duplex --seconds 10 --format csv
  .\scripts\run-windows.ps1 -Target x86_64-pc-windows-msvc -Example windows_tone -- --freq 880 --seconds 2
"@
}

# PowerShell can bind `-- ...` tails into earlier parameters.
# Normalize any option-like accidental bindings back into app args.
$rebuiltAppArgs = @()
if (-not [string]::IsNullOrWhiteSpace($Target) -and $Target.StartsWith("-")) {
    $rebuiltAppArgs += $Target
    $Target = $defaultTarget
}
if ($PSBoundParameters.ContainsKey("Bin") -and -not [string]::IsNullOrWhiteSpace($Bin) -and $Bin.StartsWith("-")) {
    $rebuiltAppArgs += $Bin
    $Bin = $null
}
if ($PSBoundParameters.ContainsKey("Example") -and -not [string]::IsNullOrWhiteSpace($Example) -and $Example.StartsWith("-")) {
    $rebuiltAppArgs += $Example
    $Example = $null
}
$rebuiltAppArgs += $AppArgs
$AppArgs = $rebuiltAppArgs

if ($AppArgs.Count -gt 0 -and $AppArgs[0] -eq "--") {
    if ($AppArgs.Count -gt 1) {
        $AppArgs = $AppArgs[1..($AppArgs.Count - 1)]
    } else {
        $AppArgs = @()
    }
}

$artifactKind = "example"
$artifactName = if ($PSBoundParameters.ContainsKey("Example") -and -not [string]::IsNullOrWhiteSpace($Example)) { $Example } else { "windows_tone" }
if ($PSBoundParameters.ContainsKey("Bin") -and -not [string]::IsNullOrWhiteSpace($Bin)) {
    $artifactKind = "bin"
    $artifactName = $Bin
}

$installedTargets = & rustup target list --installed
if (-not ($installedTargets -match "^$([Regex]::Escape($Target))$")) {
    throw "Missing Rust target $Target. Install with: rustup target add $Target"
}

$profile = if ($Release) { "release" } else { "debug" }

if (-not $NoBuild) {
    Write-Host "Building $artifactKind $artifactName for $Target ($profile)..."
    $buildArgs = @("build", "--target", $Target)
    if ($Release) { $buildArgs += "--release" }
    if ($artifactKind -eq "example") {
        $buildArgs += @("--example", $artifactName)
    } else {
        $buildArgs += @("--bin", $artifactName)
    }
    & cargo @buildArgs
}

if ($artifactKind -eq "example") {
    $localExe = Join-Path -Path "target\$Target\$profile\examples" -ChildPath "$artifactName.exe"
} else {
    $localExe = Join-Path -Path "target\$Target\$profile" -ChildPath "$artifactName.exe"
}

if (-not (Test-Path $localExe)) {
    throw "Built executable not found: $localExe"
}

Write-Host "Running Windows executable: $localExe"
if (-not [string]::IsNullOrWhiteSpace($WasapiMode)) {
    Write-Host "WASAPI mode: $WasapiMode"
    $env:TROMBONE_WASAPI_MODE = $WasapiMode
}
& $localExe @AppArgs
