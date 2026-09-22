$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not (Test-Path ".venv")) {
    py -3 -m venv .venv
}
& .\.venv\Scripts\Activate.ps1
python -m pip install -U pip
pip install -e ".[dev,desktop]"
pytest -q
pyinstaller --noconfirm --clean packaging/devicesim.spec
Write-Host "打包完成: $Root\dist\DeviceSim\DeviceSim.exe"
