#!/bin/sh
# Installs the development tools that are not Rust crates, inside the repository (.tools), on
# Linux and macOS (setup-dev.ps1 does it on Windows):
#  - libclang: generates the bindings of the C++ libraries at build time (bindgen).
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
# the library taken out of its wheel by hand: pip installs nothing into a Python the system manages
wheel=$(mktemp -d)
python3 -m pip download --disable-pip-version-check --no-deps --dest "$wheel" "libclang==18.1.1"
python3 - "$wheel"/libclang-*.whl "$root/.tools/libclang/clang/native" <<'PY'
import os, sys, zipfile

wheel, native = sys.argv[1:]
os.makedirs(native, exist_ok=True)
with zipfile.ZipFile(wheel) as files:
    for name in files.namelist():
        library = os.path.basename(name)
        if library.startswith("libclang."):
            with open(os.path.join(native, library), "wb") as unpacked:
                unpacked.write(files.read(name))
PY
rm -r "$wheel"
cd "$root/frontend"
pnpm install
