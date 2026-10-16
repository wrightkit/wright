$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Installer = Join-Path $Root "install.ps1"
$Smoke = Join-Path $Root "scripts\smoke-native.py"
$Version = (Get-Content (Join-Path $Root "version.txt") -Raw).Trim()
$Target = "x86_64-pc-windows-msvc"
$Work = Join-Path ([IO.Path]::GetTempPath()) ("wright-install-test-" + [Guid]::NewGuid().ToString("N"))
$Port = Get-Random -Minimum 18000 -Maximum 48000
$ClosedListener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$ClosedListener.Start()
$ClosedPort = $ClosedListener.LocalEndpoint.Port
$ClosedListener.Stop()
$Server = $null

function Fail([string]$Message) {
    throw "FAIL: $Message"
}

try {
    $InstallerText = Get-Content -LiteralPath $Installer -Raw
    if ($InstallerText -notmatch 'https://install\.wrightkit\.dev/wright') {
        Fail "installer must default to the install.wrightkit.dev/wright release base"
    }
    if ($InstallerText -match 'WinHttp|Http2|HTTP/2|Add-Type|DllImport') {
        Fail "installer must not embed a custom HTTP transport or negotiate HTTP/2"
    }
    if ($InstallerText -match 'Start-BitsTransfer|curl\.exe') {
        Fail "installer must not depend on external download tools"
    }
    if ($InstallerText -notmatch 'Invoke-WebRequest|WebClient|HttpClient') {
        Fail "installer must download through a native PowerShell mechanism"
    }
    $DownloadCount = [regex]::Matches($InstallerText, '(?m)^\s*Get-RemoteFile\s+\$').Count
    if ($DownloadCount -ne 3) {
        Fail "installer must route latest, archive, and checksum requests through one download path"
    }

    $VersionedRelease = Join-Path $Work "wright\releases\$Version"
    $LatestRelease = Join-Path $Work "wright\latest"
    $Payload = Join-Path $Work "payload\wright-$Version-$Target"
    New-Item -ItemType Directory -Path $VersionedRelease, $LatestRelease, $Payload -Force | Out-Null
    foreach ($Name in @("wright.exe", "wright-lsp.exe")) {
        $Source = Join-Path $Root "target\debug\$Name"
        if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) {
            Fail "missing $Source; build the Windows debug binaries before running this test"
        }
        Copy-Item -LiteralPath $Source -Destination (Join-Path $Payload $Name)
    }
    $ArchiveName = "wright-$Version-$Target.zip"
    $Archive = Join-Path $VersionedRelease $ArchiveName
    Compress-Archive -LiteralPath $Payload -DestinationPath $Archive -Force
    $Hash = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$Hash  $ArchiveName" | Set-Content -LiteralPath "${Archive}.sha256" -NoNewline -Encoding ASCII
    $Version | Set-Content -LiteralPath (Join-Path $LatestRelease "version") -NoNewline -Encoding ASCII

    $ServerCode = @'
import http.server
import os
import sys

os.chdir(sys.argv[2])
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), http.server.SimpleHTTPRequestHandler).serve_forever()
'@
    $ServerScript = Join-Path $Work "server.py"
    $ServerCode | Set-Content -LiteralPath $ServerScript -NoNewline -Encoding ASCII
    $Server = Start-Process -FilePath "python" -ArgumentList @($ServerScript, $Port, $Work) -PassThru -WindowStyle Hidden
    $BaseUrl = "http://127.0.0.1:$Port/wright"
    $LatestVersionUrl = "$BaseUrl/latest/version"
    for ($Attempt = 0; $Attempt -lt 30; $Attempt++) {
        try {
            Invoke-WebRequest -Uri $LatestVersionUrl -UseBasicParsing | Out-Null
            break
        } catch {
            if ($Attempt -eq 29) { Fail "local release server did not become ready" }
            Start-Sleep -Milliseconds 100
        }
    }

    $UnknownVersion = "0.0.0"
    $UnknownDir = Join-Path $Work "unknown"
    try {
        & $Installer -Version $UnknownVersion -InstallDir $UnknownDir -BaseUrl $BaseUrl
        Fail "unknown exact version was accepted or ignored"
    } catch {
        if ($_.Exception.Message -notmatch "failed to download") { throw }
    }
    if (Test-Path -LiteralPath (Join-Path $UnknownDir "wright.exe")) {
        Fail "unknown exact version left a partial installation"
    }
    Write-Host "PASS: exact version selection"

    $UnreachableDir = Join-Path $Work "unreachable"
    try {
        & $Installer -InstallDir $UnreachableDir -BaseUrl "http://127.0.0.1:$ClosedPort/wright"
        Fail "unreachable release server was accepted"
    } catch {
        if ($_.Exception.Message -notmatch "could not resolve the latest release") { throw }
    }
    Write-Host "PASS: unreachable release server fails latest-version resolution"

    $PinnedDir = Join-Path $Work "pinned"
    & $Installer -Version $Version -InstallDir $PinnedDir -BaseUrl $BaseUrl
    if (-not (Test-Path -LiteralPath (Join-Path $PinnedDir "wright.exe")) -or
        -not (Test-Path -LiteralPath (Join-Path $PinnedDir "wright-lsp.exe"))) {
        Fail "pinned install did not install both executables"
    }
    & python $Smoke `
        --wright (Join-Path $PinnedDir "wright.exe") `
        --wright-lsp (Join-Path $PinnedDir "wright-lsp.exe") `
        --version $Version
    if ($LASTEXITCODE -ne 0) { Fail "native post-install smoke failed" }
    Write-Host "PASS: pinned install and native smoke check"

    $LatestDir = Join-Path $Work "latest-install"
    & $Installer -InstallDir $LatestDir -BaseUrl $BaseUrl
    if (-not (Test-Path -LiteralPath (Join-Path $LatestDir "wright.exe"))) {
        Fail "latest-release install did not install wright.exe"
    }
    Write-Host "PASS: latest-release versioned R2 path resolution"

    "$(('0' * 64) -join '')  $ArchiveName" | Set-Content -LiteralPath "${Archive}.sha256" -NoNewline -Encoding ASCII
    $CorruptDir = Join-Path $Work "corrupt"
    try {
        & $Installer -Version $Version -InstallDir $CorruptDir -BaseUrl $BaseUrl
        Fail "checksum mismatch was accepted"
    } catch {
        if ($_.Exception.Message -notmatch "checksum verification failed") { throw }
    }
    if (Test-Path -LiteralPath (Join-Path $CorruptDir "wright.exe")) {
        Fail "checksum failure left a partial installation"
    }
    Write-Host "PASS: checksum mismatch is rejected before installation"

    # The checksum file matches the served bytes, so the failure must come
    # from extraction rather than checksum verification.
    "not a zip archive" | Set-Content -LiteralPath $Archive -NoNewline -Encoding ASCII
    $InvalidArchiveHash = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$InvalidArchiveHash  $ArchiveName" | Set-Content -LiteralPath "${Archive}.sha256" -NoNewline -Encoding ASCII
    $ExtractFailDir = Join-Path $Work "extract-fail"
    try {
        & $Installer -Version $Version -InstallDir $ExtractFailDir -BaseUrl $BaseUrl
        Fail "a non-ZIP archive was accepted"
    } catch {
        if ($_.Exception.Message -notmatch "failed to extract") { throw }
    }
    if (Test-Path -LiteralPath (Join-Path $ExtractFailDir "wright.exe")) {
        Fail "extraction failure left a partial installation"
    }
    Write-Host "PASS: a non-ZIP archive fails before installation"

} finally {
    if ($Server) { Stop-Process -Id $Server.Id -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $Work) { Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue }
}
