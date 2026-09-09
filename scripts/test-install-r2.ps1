$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Installer = Join-Path $Root "install.ps1"
$Work = Join-Path ([IO.Path]::GetTempPath()) ("wright-install-r2-test-" + [Guid]::NewGuid().ToString("N"))
$InstallDir = Join-Path $Work "bin"

function Fail([string]$Message) {
    throw "FAIL: $Message"
}

try {
    & $Installer -InstallDir $InstallDir
    $ExpectedExecutables = @("wright.exe", "wright-lsp.exe")
    if ($ExpectedExecutables.Count -ne 2 -or
        $ExpectedExecutables[0] -ne "wright.exe" -or
        $ExpectedExecutables[1] -ne "wright-lsp.exe") {
        Fail "production R2 smoke must cover wright.exe and wright-lsp.exe"
    }
    foreach ($Name in $ExpectedExecutables) {
        $Executable = Join-Path $InstallDir $Name
        if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
            Fail "production R2 install did not install $Name"
        }
        & $Executable --version | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Fail "production R2 install smoke failed for $Name"
        }
    }
    Write-Host "PASS: production R2 install and native smoke check"
} finally {
    if (Test-Path -LiteralPath $Work) {
        Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue
    }
}
