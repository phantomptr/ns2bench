#!/bin/sh
# Start/stop the NS2Bench Bridge daemon by pidfile (busybox-safe; never pattern-kills).
#   ns2bench-hidd.sh start [daemon args...] | stop | restart | status
DIR=$(cd "$(dirname "$0")" && pwd)
PIDFILE=/run/ns2bench-hidd.pid
ARGSFILE=/run/ns2bench-hidd.args   # last start arguments, reused by argument-less restarts
LOG=/tmp/ns2bench-hidd.log

running() {
    [ -f "$PIDFILE" ] || return 1
    pid=$(cat "$PIDFILE")
    [ -r "/proc/$pid/cmdline" ] && tr '\000' ' ' < "/proc/$pid/cmdline" | grep -q "ns2bench-board hidd"
}

case "${1:-}" in
    start)
        shift
        if running; then echo "already running (pid $(cat "$PIDFILE"))"; exit 0; fi
        [ -x "$DIR/ns2bench-board" ] || { echo "missing ns2bench-board; install the static Rust NS2Bench Bridge daemon" >&2; exit 1; }
        # A USB mode switch restarts the daemon with no arguments: keep e.g. --bind.
        if [ $# -gt 0 ]; then
            (umask 077; printf '%s\n' "$@" > "$ARGSFILE")
        elif [ -f "$ARGSFILE" ]; then
            while IFS= read -r a; do set -- "$@" "$a"; done < "$ARGSFILE"
        fi
        # Real-time priority: a busy board must never delay a button report
        # (measured p99 round trip 7.3 -> 6.3 ms on an idle board).
        RT=""
        command -v chrt >/dev/null 2>&1 && RT="chrt -f 50"
        $RT "$DIR/ns2bench-board" hidd "$@" > "$LOG" 2>&1 &
        echo $! > "$PIDFILE"
        sleep 1
        if running; then echo "started (pid $(cat "$PIDFILE"))"; else echo "failed:"; tail -5 "$LOG"; exit 1; fi
        ;;
    stop)
        if running; then kill "$(cat "$PIDFILE")"; sleep 1; fi
        rm -f "$PIDFILE"
        echo stopped
        ;;
    restart)
        shift
        "$0" stop
        "$0" start "$@"
        ;;
    status)
        if running; then echo "running (pid $(cat "$PIDFILE"))"; else echo "not running"; fi
        tail -3 "$LOG" 2>/dev/null
        ;;
    *)
        echo "usage: $0 start|stop|restart|status"; exit 2 ;;
esac
