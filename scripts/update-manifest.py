"""The update manifest (latest.json) of a release, from the signatures of its packages.

usage: update-manifest.py TAG OWNER/REPO SIGNATURES_DIR > latest.json

Each package of the release has its signature beside it (name.sig); the installed copies
fetch the manifest, find their system in it, and check the package against its signature.
"""
import datetime
import json
import sys
from pathlib import Path

# the end of a package's name, and the systems it updates: `<os>-<arch>-app` and
# `linux-x86_64-appimage` for Prisme's own updates, the others for Tauri's updater of the
# versions before 0.7 (on Windows, Prisme's installer, which it runs as their update)
PACKAGES = {
    '-windows-x64.exe': ['windows-x86_64', 'windows-x86_64-nsis'],
    '-windows-x64-update.tar.gz': ['windows-x86_64-app'],
    '-linux-x64.AppImage': ['linux-x86_64', 'linux-x86_64-appimage'],
    '-linux-x64.deb': ['linux-x86_64-deb'],
    '-macos-universal-update.tar.gz': ['darwin-aarch64', 'darwin-x86_64', 'darwin-aarch64-app', 'darwin-x86_64-app'],
}

tag, repo, folder = sys.argv[1], sys.argv[2], Path(sys.argv[3])
platforms = {}
for ending, systems in PACKAGES.items():
    signatures = [s for s in folder.glob('*.sig') if s.name.removesuffix('.sig').endswith(ending)]
    if len(signatures) != 1:
        sys.exit(f'one package ending in {ending} expected, found {[s.name for s in signatures]}')
    package = signatures[0].name.removesuffix('.sig')
    entry = {
        'signature': signatures[0].read_text(encoding='utf8'),
        'url': f'https://github.com/{repo}/releases/download/{tag}/{package}',
    }
    platforms.update({system: entry for system in systems})

now = datetime.datetime.now(datetime.timezone.utc)
manifest = {
    'version': tag,
    'notes': '',
    'pub_date': now.isoformat(timespec='milliseconds').replace('+00:00', 'Z'),
    'platforms': platforms,
}
json.dump(manifest, sys.stdout, indent=2)
