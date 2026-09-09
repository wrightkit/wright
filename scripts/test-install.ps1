$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Installer = Join-Path $Root "install.ps1"
$Smoke = Join-Path $Root "scripts\smoke-native.py"
$Version = (Get-Content (Join-Path $Root "version.txt") -Raw).Trim()
$Target = "x86_64-pc-windows-msvc"
$Work = Join-Path ([IO.Path]::GetTempPath()) ("wright-install-test-" + [Guid]::NewGuid().ToString("N"))
$Port = Get-Random -Minimum 18000 -Maximum 48000
$Server = $null

function Fail([string]$Message) {
    throw "FAIL: $Message"
}

try {
    $InstallerText = Get-Content -LiteralPath $Installer -Raw
    if ($InstallerText -notmatch 'WinHttpSetOption\(session,\s*WINHTTP_OPTION_ENABLE_HTTP_PROTOCOL' -or
        $InstallerText -notmatch 'WinHttpSetOption\(request,\s*WINHTTP_OPTION_HTTP_PROTOCOL_REQUIRED' -or
        $InstallerText -notmatch 'WinHttpQueryOption\(request,\s*WINHTTP_OPTION_HTTP_PROTOCOL_USED' -or
        $InstallerText -match 'Start-BitsTransfer|curl\.exe|Invoke-(WebRequest|RestMethod)') {
        Fail "installer must require and verify WinHTTP HTTP/2 for HTTPS release downloads"
    }
    $DownloadCount = [regex]::Matches($InstallerText, '(?m)^\s*Get-RemoteFile\s+\$').Count
    if ($DownloadCount -ne 3) {
        Fail "installer must use WinHTTP for latest, archive, and checksum requests"
    }

    $VersionedRelease = Join-Path $Work "releases\$Version"
    $LatestRelease = Join-Path $Work "latest"
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
    $BaseUrl = "http://127.0.0.1:$Port"
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

} finally {
    if ($Server) { Stop-Process -Id $Server.Id -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $Work) { Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue }
}
