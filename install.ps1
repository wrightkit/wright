[CmdletBinding()]
param(
    [string]$Version,
    [string]$InstallDir,
    [string]$BaseUrl
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

if (-not ("Wright.Http2Downloader" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace Wright {
    public static class Http2Downloader {
        const uint WINHTTP_ACCESS_TYPE_DEFAULT_PROXY = 0;
        const uint WINHTTP_FLAG_SECURE = 0x00800000;
        const uint WINHTTP_OPTION_ENABLE_HTTP_PROTOCOL = 133;
        const uint WINHTTP_OPTION_HTTP_PROTOCOL_USED = 134;
        const uint WINHTTP_OPTION_HTTP_PROTOCOL_REQUIRED = 145;
        const uint WINHTTP_PROTOCOL_FLAG_HTTP2 = 1;
        const uint WINHTTP_QUERY_STATUS_CODE = 19;
        const int ERROR_INSUFFICIENT_BUFFER = 122;

        [DllImport("winhttp.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern IntPtr WinHttpOpen(string agent, uint accessType, IntPtr proxy, IntPtr bypass, uint flags);
        [DllImport("winhttp.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern IntPtr WinHttpConnect(IntPtr session, string server, ushort port, uint reserved);
        [DllImport("winhttp.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern IntPtr WinHttpOpenRequest(IntPtr connection, string verb, string objectName, string version, string referrer, IntPtr acceptTypes, uint flags);
        [DllImport("winhttp.dll", SetLastError = true)]
        static extern bool WinHttpSetOption(IntPtr handle, uint option, ref uint buffer, uint bufferLength);
        [DllImport("winhttp.dll", SetLastError = true)]
        static extern bool WinHttpQueryOption(IntPtr handle, uint option, out uint buffer, ref uint bufferLength);
        [DllImport("winhttp.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern bool WinHttpQueryHeaders(IntPtr request, uint infoLevel, string name, StringBuilder buffer, ref uint bufferLength, IntPtr index);
        [DllImport("winhttp.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern bool WinHttpSendRequest(IntPtr request, string headers, uint headersLength, IntPtr optional, uint optionalLength, uint totalLength, IntPtr context);
        [DllImport("winhttp.dll", SetLastError = true)]
        static extern bool WinHttpReceiveResponse(IntPtr request, IntPtr reserved);
        [DllImport("winhttp.dll", SetLastError = true)]
        static extern bool WinHttpQueryDataAvailable(IntPtr request, out uint available);
        [DllImport("winhttp.dll", SetLastError = true)]
        static extern bool WinHttpReadData(IntPtr request, [Out] byte[] buffer, uint bytesToRead, out uint bytesRead);
        [DllImport("winhttp.dll", SetLastError = true)]
        static extern bool WinHttpCloseHandle(IntPtr handle);

        static void Check(bool success, string operation) {
            if (!success) throw new Win32Exception(Marshal.GetLastWin32Error(), operation);
        }

        static uint StatusCode(IntPtr request) {
            uint length = 0;
            WinHttpQueryHeaders(request, WINHTTP_QUERY_STATUS_CODE, null, null, ref length, IntPtr.Zero);
            if (Marshal.GetLastWin32Error() != ERROR_INSUFFICIENT_BUFFER) {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "WinHttpQueryHeaders");
            }
            var value = new StringBuilder((int)(length / 2));
            Check(WinHttpQueryHeaders(request, WINHTTP_QUERY_STATUS_CODE, null, value, ref length, IntPtr.Zero), "WinHttpQueryHeaders");
            return UInt32.Parse(value.ToString());
        }

        public static void Download(string address, string destination) {
            var uri = new Uri(address);
            if (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps) {
                throw new ArgumentException("only HTTP(S) URLs are supported", "address");
            }
            IntPtr session = IntPtr.Zero;
            IntPtr connection = IntPtr.Zero;
            IntPtr request = IntPtr.Zero;
            try {
                session = WinHttpOpen("wright-installer", WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, IntPtr.Zero, IntPtr.Zero, 0);
                if (session == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "WinHttpOpen");
                uint http2 = WINHTTP_PROTOCOL_FLAG_HTTP2;
                Check(WinHttpSetOption(session, WINHTTP_OPTION_ENABLE_HTTP_PROTOCOL, ref http2, sizeof(uint)), "WinHttpSetOption(HTTP/2)");
                connection = WinHttpConnect(session, uri.Host, (ushort)uri.Port, 0);
                if (connection == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "WinHttpConnect");
                uint flags = uri.Scheme == Uri.UriSchemeHttps ? WINHTTP_FLAG_SECURE : 0;
                request = WinHttpOpenRequest(connection, "GET", uri.PathAndQuery, null, null, IntPtr.Zero, flags);
                if (request == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "WinHttpOpenRequest");
                if (uri.Scheme == Uri.UriSchemeHttps) {
                    Check(WinHttpSetOption(request, WINHTTP_OPTION_HTTP_PROTOCOL_REQUIRED, ref http2, sizeof(uint)), "WinHttpSetOption(require HTTP/2)");
                }
                Check(WinHttpSendRequest(request, null, 0, IntPtr.Zero, 0, 0, IntPtr.Zero), "WinHttpSendRequest");
                Check(WinHttpReceiveResponse(request, IntPtr.Zero), "WinHttpReceiveResponse");
                if (uri.Scheme == Uri.UriSchemeHttps) {
                    uint length = sizeof(uint);
                    uint used;
                    Check(WinHttpQueryOption(request, WINHTTP_OPTION_HTTP_PROTOCOL_USED, out used, ref length), "WinHttpQueryOption(HTTP protocol)");
                    if (used != WINHTTP_PROTOCOL_FLAG_HTTP2) throw new InvalidOperationException("WinHTTP did not negotiate HTTP/2");
                }
                uint status = StatusCode(request);
                if (status < 200 || status >= 300) throw new InvalidOperationException("HTTP status " + status);
                using (var output = new FileStream(destination, FileMode.Create, FileAccess.Write, FileShare.None)) {
                    while (true) {
                        uint available;
                        Check(WinHttpQueryDataAvailable(request, out available), "WinHttpQueryDataAvailable");
                        if (available == 0) break;
                        var buffer = new byte[(int)available];
                        uint read;
                        Check(WinHttpReadData(request, buffer, available, out read), "WinHttpReadData");
                        if (read == 0) throw new InvalidOperationException("WinHttpReadData returned no data");
                        output.Write(buffer, 0, (int)read);
                    }
                }
            } finally {
                if (request != IntPtr.Zero) WinHttpCloseHandle(request);
                if (connection != IntPtr.Zero) WinHttpCloseHandle(connection);
                if (session != IntPtr.Zero) WinHttpCloseHandle(session);
            }
        }
    }
}
'@
}

function Get-RemoteFile([string]$Uri, [string]$Destination) {
    [Wright.Http2Downloader]::Download($Uri, $Destination)
}

function Get-Version([string]$RequestedVersion, [string]$ReleaseBaseUrl) {
    if ($RequestedVersion) {
        $resolved = $RequestedVersion.TrimStart("v")
    } else {
        $latestVersionUrl = "$($ReleaseBaseUrl.TrimEnd('/'))/latest/version"
        $latestVersionPath = Join-Path ([IO.Path]::GetTempPath()) ("wright-latest-" + [Guid]::NewGuid().ToString("N"))
        try {
            Get-RemoteFile $latestVersionUrl $latestVersionPath
            $content = Get-Content -LiteralPath $latestVersionPath -Raw
            $resolved = $content.Trim().TrimStart("v")
            if (-not $resolved) {
                Fail "latest version response from $latestVersionUrl was empty; pin a version with -Version"
            }
        } catch {
            if ($_.Exception.Message -like "error: latest version response*") { throw }
            Fail "could not resolve the latest release from $latestVersionUrl: $($_.Exception.Message); pin a version with -Version"
        } finally {
            if (Test-Path -LiteralPath $latestVersionPath) {
                Remove-Item -LiteralPath $latestVersionPath -Force -ErrorAction SilentlyContinue
            }
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

$BaseUrl = Resolve-Setting $BaseUrl "WRIGHT_INSTALL_BASE_URL" "https://releases.wrightkit.dev"
$Version = Get-Version $Version $BaseUrl
if (-not $InstallDir) {
    $InstallRoot = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:USERPROFILE }
    if (-not $InstallRoot) { Fail "could not determine a user-writable install directory; pass -InstallDir" }
    $InstallDir = Join-Path $InstallRoot "Programs\Wright\bin"
}

$ArchiveName = "wright-$Version-$Target.zip"
$ArchiveUrl = "$($BaseUrl.TrimEnd('/'))/releases/$Version/$ArchiveName"
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
        Get-RemoteFile $ArchiveUrl $ArchivePath
        Get-RemoteFile $ChecksumUrl $ChecksumPath
    } catch {
        Fail "failed to download the release archive or checksum for v$Version from $BaseUrl: $($_.Exception.Message)"
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
