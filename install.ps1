[CmdletBinding()]
param(
    [string]$Version,
    [string]$InstallDir,
    [string]$BaseUrl,
    [string]$ApiUrl
)

$ErrorActionPreference = "Stop"
$Target = "x86_64-pc-windows-msvc"

function Fail([string]$Message) {
    throw "error: $Message"
}

function Resolve-Setting([string]$Value, [string]$EnvironmentName, [string]$Default) {
    if ($Value) { return $Value }
    $environmentValue = [Environment]::GetEnvironmentVariable($EnvironmentName)
    if ($environmentValue) { return $environmentValue }
    return $Default
}

function Get-Version([string]$RequestedVersion, [string]$ReleaseApiUrl) {
    if ($RequestedVersion) {
        $resolved = $RequestedVersion.TrimStart("v")
    } else {
        try {
            $release = Invoke-RestMethod -Uri $ReleaseApiUrl -Headers @{ "User-Agent" = "wright-installer" }
            $resolved = ([string]$release.tag_name).TrimStart("v")
            if (-not $resolved) {
                Fail "latest release response from $ReleaseApiUrl did not contain a tag; pin a version with -Version"
            }
        } catch {
            if ($_.Exception.Message -like "error: latest release response*") { throw }
            Fail "could not resolve the latest release from $ReleaseApiUrl; pin a version with -Version"
        }
    }
    if ($resolved -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$') {
        Fail "invalid version '$resolved' (expected semver like 0.1.0)"
    }
    return $resolved
}

function Get-Checksum([string]$ChecksumPath, [string]$ArchiveName) {
    $line = Get-Content -LiteralPath $ChecksumPath | Where-Object { $_.Trim() } | Select-Object -First 1
    if ($line -notmatch '^\s*([0-9a-fA-F]{64})\s+\*?([^\s]+)\s*$') {
        Fail "invalid checksum file for $ArchiveName"
    }
    if ([IO.Path]::GetFileName($Matches[2]) -ne $ArchiveName) {
        Fail "checksum file names '$($Matches[2])', not '$ArchiveName'"
    }
    return $Matches[1].ToUpperInvariant()
}

function Test-Version([string]$Executable, [string]$Version) {
    try {
        $output = (& $Executable --version 2>&1 | Out-String).Trim()
    } catch {
        Fail "post-install smoke check could not execute '$Executable'"
    }
    if ($LASTEXITCODE -ne 0 -or $output -notlike "*$Version*") {
        Fail "post-install smoke check for '$Executable' did not report version $Version"
    }
}

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    Fail "unsupported operating system; install.ps1 must run on Windows x86_64"
}
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne [Runtime.InteropServices.Architecture]::X64) {
    Fail "unsupported CPU architecture; install.ps1 supports Windows x86_64 only"
}

$BaseUrl = Resolve-Setting $BaseUrl "WRIGHT_INSTALL_BASE_URL" "https://github.com/wrightkit/wright/releases/download"
$ApiUrl = Resolve-Setting $ApiUrl "WRIGHT_API_URL" "https://api.github.com/repos/wrightkit/wright/releases/latest"
$Version = Get-Version $Version $ApiUrl
if (-not $InstallDir) {
    $InstallRoot = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:USERPROFILE }
    if (-not $InstallRoot) { Fail "could not determine a user-writable install directory; pass -InstallDir" }
    $InstallDir = Join-Path $InstallRoot "Programs\Wright\bin"
}

$ArchiveName = "wright-$Version-$Target.zip"
$ArchiveUrl = "$($BaseUrl.TrimEnd('/'))/v$Version/$ArchiveName"
$ChecksumUrl = "${ArchiveUrl}.sha256"
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ("wright-install-" + [Guid]::NewGuid().ToString("N"))
$ExtractDir = Join-Path $TempRoot "extract"
$StageDir = Join-Path $TempRoot "stage"

try {
    New-Item -ItemType Directory -Path $TempRoot, $ExtractDir, $StageDir -Force | Out-Null
    $ArchivePath = Join-Path $TempRoot $ArchiveName
    $ChecksumPath = "$ArchivePath.sha256"
    Write-Host "==> downloading $ArchiveUrl"
    try {
        Invoke-WebRequest -Uri $ArchiveUrl -OutFile $ArchivePath -UseBasicParsing
        Invoke-WebRequest -Uri $ChecksumUrl -OutFile $ChecksumPath -UseBasicParsing
    } catch {
        Fail "failed to download the release archive or checksum for v$Version from $BaseUrl"
    }

    Write-Host "==> verifying SHA-256 checksum"
    $ExpectedHash = Get-Checksum $ChecksumPath $ArchiveName
    $ActualHash = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash.ToUpperInvariant()
    if ($ActualHash -ne $ExpectedHash) {
        Fail "checksum verification failed for $ArchiveName; the download may be corrupted or tampered with, so nothing was installed"
    }

    Write-Host "==> extracting release archive"
    try {
        Expand-Archive -LiteralPath $ArchivePath -DestinationPath $ExtractDir -Force
    } catch {
        Fail "failed to extract $ArchiveName"
    }
    $PayloadDir = Join-Path $ExtractDir "wright-$Version-$Target"
    $Wright = Join-Path $PayloadDir "wright.exe"
    $Lsp = Join-Path $PayloadDir "wright-lsp.exe"
    if (-not (Test-Path -LiteralPath $PayloadDir -PathType Container) -or
        -not (Test-Path -LiteralPath $Wright -PathType Leaf) -or
        -not (Test-Path -LiteralPath $Lsp -PathType Leaf)) {
        Fail "unexpected archive layout; expected $PayloadDir with wright.exe and wright-lsp.exe"
    }

    Copy-Item -LiteralPath $Wright -Destination (Join-Path $StageDir "wright.exe")
    Copy-Item -LiteralPath $Lsp -Destination (Join-Path $StageDir "wright-lsp.exe")
    Test-Version (Join-Path $StageDir "wright.exe") $Version
    Test-Version (Join-Path $StageDir "wright-lsp.exe") $Version

    Write-Host "==> installing into $InstallDir"
    try {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        $WriteProbe = Join-Path $InstallDir (".wright-write-test-" + [Guid]::NewGuid().ToString("N"))
        New-Item -ItemType File -Path $WriteProbe -Force | Out-Null
        Remove-Item -LiteralPath $WriteProbe -Force
    } catch {
        Fail "install directory '$InstallDir' is not writable; choose a writable location with -InstallDir"
    }
    Copy-Item -LiteralPath (Join-Path $StageDir "wright.exe") -Destination (Join-Path $InstallDir "wright.exe") -Force
    Copy-Item -LiteralPath (Join-Path $StageDir "wright-lsp.exe") -Destination (Join-Path $InstallDir "wright-lsp.exe") -Force
    Test-Version (Join-Path $InstallDir "wright.exe") $Version
    Test-Version (Join-Path $InstallDir "wright-lsp.exe") $Version

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $PathEntry = $InstallDir.TrimEnd("\")
    $PathEntries = if ($UserPath) { $UserPath -split ";" } else { @() }
    $PathContainsInstallDir = @($PathEntries | Where-Object {
        $_.Trim().TrimEnd("\") -ieq $PathEntry
    }).Count -gt 0
    if (-not $PathContainsInstallDir) {
        $DisplayPath = $InstallDir.Replace("'", "''")
        Write-Host "note: '$InstallDir' is not on your user PATH; add it and open a new terminal to use wright by name:"
        Write-Host "  `$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')"
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', (`$userPath.TrimEnd(';') + ';$DisplayPath'), 'User')"
    }
    Write-Host "==> done: wright and wright-lsp $Version installed in $InstallDir"
} finally {
    if (Test-Path -LiteralPath $TempRoot) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
