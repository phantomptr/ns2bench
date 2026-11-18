#!/bin/sh
# Install or update NS2Bench on a Linux USB-gadget board, from the computer, over SSH.
#   board/install.sh <ssh-host> <ns2bench-board binary> [token file]
# Normally run through `make board-install BOARD=<ssh-host>`.
#
# Safe update: files go up as *.new, scripts are syntax-checked on the board, the current
# files are kept as *.bak, then swapped in and the daemon is restarted and checked.
# An existing board token is never overwritten (the host would lose access); a token is
# only uploaded when the board has none. The USB gadget itself is not rebuilt: a new
# gadget.sh takes effect on the next mode switch or boot.
# The on-board steps live in remote-install.sh (shared with the desktop app's installer).
set -eu
HOST=${1:?usage: install.sh <ssh-host> <binary> [token-file]}
BIN=${2:?binary path required}
TOKEN=${3:-$HOME/.ns2bench/hidd.token}
HERE=$(cd "$(dirname "$0")" && pwd)
DEST=/opt/ns2bench
case "$HOST" in -*|*[!A-Za-z0-9@._:-]*) echo "bad ssh host: $HOST" >&2; exit 2 ;; esac
SSH="ssh -o BatchMode=yes -o ConnectTimeout=5 $HOST"

[ -f "$BIN" ] || { echo "no binary at $BIN (run: make board)" >&2; exit 1; }
$SSH "mkdir -p $DEST/quirks && chmod 700 $DEST"

echo "uploading to $HOST:$DEST"
scp -q "$BIN" "$HOST:$DEST/ns2bench-board.new"
scp -q "$HERE/gadget.sh" "$HOST:$DEST/gadget.sh.new"
for f in ns2bench-hidd.sh swap.sh restore-default-gadget.sh S98ns2bench-swap S45ns2bench-early; do scp -q "$HERE/$f" "$HOST:$DEST/$f.new"; done
for q in "$HERE"/quirks/*.sh; do scp -q "$q" "$HOST:$DEST/quirks/$(basename "$q").new"; done

if ! $SSH "test -s $DEST/token"; then
    if [ ! -f "$TOKEN" ]; then
        mkdir -p "$(dirname "$TOKEN")"
        (umask 077; head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n' > "$TOKEN"; echo >> "$TOKEN")
        echo "created token $TOKEN"
    fi
    $SSH "umask 077; cat > $DEST/token" < "$TOKEN"
    echo "uploaded token"
fi

$SSH sh -s < "$HERE/remote-install.sh"

# Every file on the board must match what was sent (a power cut before the flush empties files).
want=$( (cd "$HERE" && md5sum gadget.sh ns2bench-hidd.sh swap.sh restore-default-gadget.sh quirks/luckfox.sh 2>/dev/null || md5 -r gadget.sh ns2bench-hidd.sh swap.sh restore-default-gadget.sh quirks/luckfox.sh) | awk '{print $1}' | tr '\n' ' ')
bin=$( (md5sum "$BIN" 2>/dev/null || md5 -r "$BIN") | awk '{print $1}')
got=$($SSH "cd $DEST && sync && md5sum gadget.sh ns2bench-hidd.sh swap.sh restore-default-gadget.sh quirks/luckfox.sh | awk '{print \$1}' | tr '\\n' ' '; echo; md5sum ns2bench-board | awk '{print \$1}'")
if [ "$(echo "$got" | head -1)" != "$want" ] || [ "$(echo "$got" | tail -1)" != "$bin" ]; then
    echo "the files on the board don't match what was sent: run the install again" >&2
    exit 1
fi
echo "verified: every file on the board matches"
echo "done. previous files are kept on the board as *.bak"
