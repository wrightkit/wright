Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$scoopShims = Join-Path $env:USERPROFILE "scoop\shims"
Add-Content -Path $env:GITHUB_PATH -Value $scoopShims
$env:PATH = "$scoopShims;$env:PATH"
scoop --version
