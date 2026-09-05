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
    $Release = Join-Path $Work "v$Version"
    $Payload = Join-Path $Release "wright-$Version-$Target"
    New-Item -ItemType Directory -Path $Payload -Force | Out-Null
    foreach ($Name in @("wright.exe", "wright-lsp.exe")) {
        $Source = Join-Path $Root "target\debug\$Name"
        if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) {
            Fail "missing $Source; build the Windows debug binaries before running this test"
        }
        Copy-Item -LiteralPath $Source -Destination (Join-Path $Payload $Name)
    }
    $ArchiveName = "wright-$Version-$Target.zip"
    $Archive = Join-Path $Release $ArchiveName
    Compress-Archive -LiteralPath $Payload -DestinationPath $Archive -Force
    $Hash = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$Hash  $ArchiveName" | Set-Content -LiteralPath "${Archive}.sha256" -NoNewline -Encoding ASCII

    $ApiDirectory = Join-Path $Work "repos\wrightkit\wright\releases"
    New-Item -ItemType Directory -Path $ApiDirectory -Force | Out-Null
    '{"tag_name":"v' + $Version + '","draft":false,"prerelease":false}' |
        Set-Content -LiteralPath (Join-Path $ApiDirectory "latest") -NoNewline -Encoding ASCII
    $ServerCode = @'
import http.server
import os
import sys

class Handler(http.server.SimpleHTTPRequestHandler):
    def guess_type(self, path):
        if path.endswith("/latest"):
            return "application/json"
        return super().guess_type(path)

os.chdir(sys.argv[2])
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
'@
    $ServerScript = Join-Path $Work "server.py"
    $ServerCode | Set-Content -LiteralPath $ServerScript -NoNewline -Encoding ASCII
    $Server = Start-Process -FilePath "python" -ArgumentList @($ServerScript, $Port, $Work) -PassThru -WindowStyle Hidden
    $BaseUrl = "http://127.0.0.1:$Port"
    $ApiUrl = "$BaseUrl/repos/wrightkit/wright/releases/latest"
    for ($Attempt = 0; $Attempt -lt 30; $Attempt++) {
        try {
            Invoke-RestMethod -Uri $ApiUrl | Out-Null
            break
        } catch {
            if ($Attempt -eq 29) { Fail "local release server did not become ready" }
            Start-Sleep -Milliseconds 100
        }
    }

    $UnknownVersion = "0.0.0"
    $UnknownDir = Join-Path $Work "unknown"
    try {
        & $Installer -Version $UnknownVersion -InstallDir $UnknownDir -BaseUrl $BaseUrl -ApiUrl $ApiUrl
        Fail "unknown exact version was accepted or ignored"
    } catch {
        if ($_.Exception.Message -notmatch "failed to download") { throw }
    }
    if (Test-Path -LiteralPath (Join-Path $UnknownDir "wright.exe")) {
        Fail "unknown exact version left a partial installation"
    }
    Write-Host "PASS: exact version selection"

    $PinnedDir = Join-Path $Work "pinned"
    & $Installer -Version $Version -InstallDir $PinnedDir -BaseUrl $BaseUrl -ApiUrl $ApiUrl
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

    $LatestDir = Join-Path $Work "latest"
    & $Installer -InstallDir $LatestDir -BaseUrl $BaseUrl -ApiUrl $ApiUrl
    if (-not (Test-Path -LiteralPath (Join-Path $LatestDir "wright.exe"))) {
        Fail "latest-release install did not install wright.exe"
    }
    Write-Host "PASS: latest-release resolution"

    "$(('0' * 64) -join '')  $ArchiveName" | Set-Content -LiteralPath "${Archive}.sha256" -NoNewline -Encoding ASCII
    $CorruptDir = Join-Path $Work "corrupt"
    try {
        & $Installer -Version $Version -InstallDir $CorruptDir -BaseUrl $BaseUrl -ApiUrl $ApiUrl
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
