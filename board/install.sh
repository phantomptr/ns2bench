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
for f in ns2bench-hidd.sh swap.sh restore-default-gadget.sh S98ns2bench-swap; do scp -q "$HERE/$f" "$HOST:$DEST/$f.new"; done
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

$SSH "set -e; cd $DEST
for f in gadget.sh.new ns2bench-hidd.sh.new swap.sh.new S98ns2bench-swap.new quirks/*.new; do sh -n \"\$f\"; done
bash -n restore-default-gadget.sh.new
chmod 755 ns2bench-board.new
./ns2bench-board.new version
for f in ns2bench-board gadget.sh ns2bench-hidd.sh; do [ ! -e \"\$f\" ] || cp -p \"\$f\" \"\$f.bak\"; done
for q in quirks/*.new; do b=\${q%.new}; [ ! -e \"\$b\" ] || cp -p \"\$b\" \"\$b.bak\"; mv \"\$q\" \"\$b\"; done
for f in swap.sh restore-default-gadget.sh; do [ ! -e \"\$f\" ] || cp -p \"\$f\" \"\$f.bak\"; done
chmod 755 ns2bench-board.new gadget.sh.new ns2bench-hidd.sh.new swap.sh.new restore-default-gadget.sh.new S98ns2bench-swap.new
mv swap.sh.new swap.sh
mv restore-default-gadget.sh.new restore-default-gadget.sh
# boot hook for SD swap: installed once, never replaced
if [ -e /etc/init.d/S98ns2bench-swap ]; then rm -f S98ns2bench-swap.new; else mv S98ns2bench-swap.new /etc/init.d/S98ns2bench-swap; fi
mv gadget.sh.new gadget.sh
mv ns2bench-hidd.sh.new ns2bench-hidd.sh
./ns2bench-hidd.sh stop >/dev/null
mv ns2bench-board.new ns2bench-board
./ns2bench-hidd.sh start"
echo "done. previous files are kept on the board as *.bak"
