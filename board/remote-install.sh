#!/bin/sh
# Runs ON the board (piped over SSH: `ssh <host> sh -s < remote-install.sh`), after the new
# files were uploaded to /opt/ns2bench as *.new. Used by board/install.sh and by the app's
# Settings → Install → Bridge board. Syntax-checks everything first, keeps the current
# files as *.bak, swaps the new ones in and restarts the daemon.
set -e
cd /opt/ns2bench
for f in gadget.sh.new ns2bench-hidd.sh.new swap.sh.new S98ns2bench-swap.new quirks/*.new; do sh -n "$f"; done
bash -n restore-default-gadget.sh.new
chmod 755 ns2bench-board.new
./ns2bench-board.new version
for f in ns2bench-board gadget.sh ns2bench-hidd.sh; do [ ! -e "$f" ] || cp -p "$f" "$f.bak"; done
for q in quirks/*.new; do b=${q%.new}; [ ! -e "$b" ] || cp -p "$b" "$b.bak"; mv "$q" "$b"; done
for f in swap.sh restore-default-gadget.sh; do [ ! -e "$f" ] || cp -p "$f" "$f.bak"; done
chmod 755 ns2bench-board.new gadget.sh.new ns2bench-hidd.sh.new swap.sh.new restore-default-gadget.sh.new S98ns2bench-swap.new
mv swap.sh.new swap.sh
mv restore-default-gadget.sh.new restore-default-gadget.sh
# boot hook for SD swap: installed once, never replaced
if [ -e /etc/init.d/S98ns2bench-swap ]; then rm -f S98ns2bench-swap.new; else mv S98ns2bench-swap.new /etc/init.d/S98ns2bench-swap; fi
mv gadget.sh.new gadget.sh
mv ns2bench-hidd.sh.new ns2bench-hidd.sh
./ns2bench-hidd.sh stop >/dev/null
mv ns2bench-board.new ns2bench-board
./ns2bench-hidd.sh start
