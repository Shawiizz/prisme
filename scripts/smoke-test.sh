#!/bin/bash
# Starts a build of the application once, silent, a track playing on the first deck under a
# simulated turntable, and keeps a screenshot and its logs: what a build does on a system
# nobody has at hand. Fails if the application did not stay up.
# usage: smoke-test.sh <executable> <folder for what is kept>
set -eu
exe=$1
kept=$2
mkdir -p "$kept"
data=$(mktemp -d)
python=$(command -v python3 || command -v python)

"$python" - "$data/clicks.wav" <<'PY'
import math, struct, sys, wave

RATE = 44100
frames = bytearray()
for n in range(RATE * 60):
    t = (n % (RATE // 2)) / RATE
    sample = int(20000 * math.exp(-t * 60) * math.sin(2 * math.pi * 1000 * t))
    frames += struct.pack("<hh", sample, sample)
with wave.open(sys.argv[1], "wb") as clicks:
    clicks.setnchannels(2)
    clicks.setsampwidth(2)
    clicks.setframerate(RATE)
    clicks.writeframes(frames)
PY

system=$(uname)
case $system in
Linux)
    logs=$HOME/.local/share/com.shawiizz.prisme/logs
    Xvfb :99 -screen 0 1600x1000x24 >/dev/null 2>&1 &
    sleep 2
    export DISPLAY=:99 GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1
    # a session bus of its own: GTK waits a long time for one that does not answer
    start() { dbus-run-session -- "$@"; }
    screenshot() { import -window root "$1"; }
    ;;
Darwin)
    logs=$HOME/Library/Logs/com.shawiizz.prisme
    start() { "$@"; }
    screenshot() { screencapture -x "$1"; }
    ;;
*)
    logs=$(cygpath "$LOCALAPPDATA")/com.shawiizz.prisme/logs
    data=$(cygpath -w "$data")
    start() { "$@"; }
    screenshot() {
        powershell -NoProfile -Command "
            Add-Type -AssemblyName System.Windows.Forms, System.Drawing
            \$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
            \$image = New-Object System.Drawing.Bitmap \$screen.Width, \$screen.Height
            [System.Drawing.Graphics]::FromImage(\$image).CopyFromScreen(\$screen.Location, [System.Drawing.Point]::Empty, \$screen.Size)
            \$image.Save('$(cygpath -w "$1")')"
    }
    ;;
esac

export PRISME_DATA_DIR=$data PRISME_MASTER_GAIN=0 PRISME_SIMULATE=1
case $system in
Linux | Darwin) export PRISME_DECK1=$data/clicks.wav ;;
*) export PRISME_DECK1=$data\\clicks.wav ;;
esac

start "$exe" >"$kept/output.log" 2>&1 &
app=$!
sleep 45
screenshot "$kept/screen.png" || echo "no screenshot"
up=0
kill -0 $app 2>/dev/null || up=1
kill $app 2>/dev/null || true
cp -r "$logs" "$kept/logs" 2>/dev/null || echo "no log files in $logs"
tail -n 40 "$kept/output.log"
exit $up
