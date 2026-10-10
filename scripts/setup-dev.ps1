# Installs the development tools that are not Rust crates, inside the repository (.tools).
#  - libclang: generates the ASIO bindings at build time (cpal's ASIO backend uses bindgen).
# The ASIO SDK itself is downloaded by the build (asio-sys).

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root ".tools\libclang"

if (Test-Path (Join-Path $target "clang\native\libclang.dll")) {
    Write-Host "libclang already installed in $target"
} else {
    python -m pip install --disable-pip-version-check --target $target "libclang==18.1.1"
}

Push-Location (Join-Path $root "frontend")
pnpm install
Pop-Location
