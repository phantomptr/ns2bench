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
# boot hook for SD swap: installed once, never replaced; a board without /etc/init.d goes without
if [ -e /etc/init.d/S98ns2bench-swap ] || [ ! -d /etc/init.d ]; then rm -f S98ns2bench-swap.new
else chmod 755 S98ns2bench-swap.new; mv S98ns2bench-swap.new /etc/init.d/S98ns2bench-swap || rm -f S98ns2bench-swap.new; fi
for q in quirks/*.new; do b=${q%.new}; [ ! -e "$b" ] || cp -p "$b" "$b.bak"; mv "$q" "$b"; done
for f in swap.sh restore-default-gadget.sh; do [ ! -e "$f" ] || cp -p "$f" "$f.bak"; done
chmod 755 ns2bench-board.new gadget.sh.new ns2bench-hidd.sh.new swap.sh.new restore-default-gadget.sh.new
mv swap.sh.new swap.sh
mv restore-default-gadget.sh.new restore-default-gadget.sh
mv gadget.sh.new gadget.sh
mv ns2bench-hidd.sh.new ns2bench-hidd.sh
./ns2bench-hidd.sh stop >/dev/null
mv ns2bench-board.new ns2bench-board
if ! ./ns2bench-hidd.sh start; then
  # Never leave the board without a working Bridge: put the previous version back.
  echo "the new Bridge did not start: putting the previous version back" >&2
  for f in ns2bench-board gadget.sh ns2bench-hidd.sh; do [ ! -e "$f.bak" ] || cp -p "$f.bak" "$f"; done
  ./ns2bench-hidd.sh start || true
  exit 1
fi
