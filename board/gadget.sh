#!/bin/bash
# NS2Bench USB gadget for any Linux board with USB device mode (configfs + libcomposite).
#
#   gadget.sh [standard|hori|pro|status]   default: the mode in /etc/ns2bench-usb-mode, else standard
#
#   standard  HORI-style controller (hidg0) + keyboard (hidg1) + mouse (hidg2)
#   hori      HORI-style controller + keyboard
#   pro       Switch 2 Pro Controller over FunctionFS (ns2bench-board pro2): every button incl. C/GL/GR
#
# Board-specific steps live in quirks/<name>.sh and are sourced when the board matches
# (e.g. quirks/luckfox.sh on Luckfox Pico boards). An explicit mode becomes the boot default.
set -euo pipefail

DIR=$(cd "$(dirname "$0")" && pwd)
MODE_FILE=/etc/ns2bench-usb-mode
CONFIGFS=/sys/kernel/config/usb_gadget
FFS=/dev/ffs-pro2
PRO_PID=/run/ns2bench-pro2.pid
ACTIVE_MODE=/run/ns2bench-usb-mode     # the mode really running (a failed pro falls back to standard)
LOG=/tmp/ns2bench-gadget.log
SPEED_SAVE=/run/ns2bench-max-speed
mode=${1:-$(cat "$MODE_FILE" 2>/dev/null || true)}
# The saved mode can be lost (an empty file after the console cut the board's power mid-write):
# start as standard then, never fail the boot over it.
if [ -z "${1:-}" ]; then
    case "$mode" in standard|hori|pro|pro1) ;; *) echo "saved mode '${mode}' unreadable: starting as standard" >&2; mode=standard ;; esac
fi
# Atomic write: a power cut leaves the old file or the new one, never an empty one.
save_file() { printf '%s\n' "$2" > "$1.tmp" && sync && mv -f "$1.tmp" "$1" && sync; }

# At boot (no mode given) this never fails: a failure here made the ns2-dev boot switch the
# controller mode off for good (seen three times when the console's power came and went). Log it,
# keep the mode on, and the next boot tries again. Kept in a history that survives restarts.
HISTORY=/opt/ns2bench/boot-history.log
history() {            # never fails (it runs inside the boot trap; a full or read-only disk must not matter)
    # One line added per boot. Only when it has grown long is it cut back, through a temp file that
    # is on storage before it replaces the log (rewriting it on every boot emptied it whenever the
    # power went right after: the very boots it is there to record).
    { echo "$(date '+%F %T' 2>/dev/null) up=$(cut -d' ' -f1 /proc/uptime)s $*" >> "$HISTORY"; } 2>/dev/null || true
    if [ "$(wc -l < "$HISTORY" 2>/dev/null || echo 0)" -gt 400 ] 2>/dev/null; then
        { tail -n 200 "$HISTORY" > "$HISTORY.tmp" && sync && mv -f "$HISTORY.tmp" "$HISTORY"; } 2>/dev/null || true
    fi
    return 0
}
# One build at a time: boot's second run, a switch asked by the bench and a run by hand must not
# take the USB side apart under each other. A lock left by a run that died is taken over after 30 s.
LOCK=/run/ns2bench-gadget.lock
LOCKED=0
lock() {
    local i=0 owner
    while ! mkdir "$LOCK" 2>/dev/null; do
        owner=$(cat "$LOCK/pid" 2>/dev/null || true)
        i=$((i + 1))
        # its owner is gone (stopped, or the power went mid-build on a board whose /run is kept): take over now
        if { [ -n "$owner" ] && [ ! -d "/proc/$owner" ]; } || [ "$i" -gt 150 ]; then
            echo "taking over a build lock left behind" >&2
            rm -rf "$LOCK"; i=0; continue
        fi
        sleep 0.2
    done
    echo $$ > "$LOCK/pid"
    LOCKED=1
}
# (only its owner removes it: a run that lost the lock to a takeover must not remove the taker's)
unlock() {
    if [ "$LOCKED" = 1 ]; then
        if [ "$(cat "$LOCK/pid" 2>/dev/null || true)" = "$$" ]; then rm -rf "$LOCK"; fi
        LOCKED=0
    fi
}
# Asked to stop (the Bridge gives a switch 25 s): leave through the EXIT trap, which unlocks.
trap 'exit 143' TERM INT
if [ -z "${1:-}" ]; then
    trap 'rc=$?; unlock; if [ "$rc" != 0 ]; then echo "boot: could not build the controller (exit $rc); controller mode stays on, the next boot tries again" >&2; history "gadget.sh boot run FAILED (exit $rc), kept controller mode" || true; exit 0; fi' EXIT
else
    trap 'unlock' EXIT
fi

# Every mode change is logged (at boot nothing else keeps the output), with the seconds since boot.
# Done before anything else is set up, so that work is not done twice.
if [ "$mode" != status ] && [ "${NS2BENCH_GADGET_LOGGED:-}" != 1 ]; then
    export NS2BENCH_GADGET_LOGGED=1
    # run again with the output going to the log as well, and keep its exit status
    rc_file=/run/ns2bench-gadget.rc.$$
    { "$0" "$@" 2>&1; echo $? > "$rc_file"; } | tee -a "$LOG"
    rc=$(cat "$rc_file" 2>/dev/null || echo 1); rm -f "$rc_file"
    exit "$rc"
fi


# quirk hooks (overridden by quirks/*.sh)
GADGET_NAME=ns2bench               # a quirk may reuse a vendor gadget directory instead
quirk_before() { :; }
quirk_after() { :; }
MODEL=$(tr -d '\0' < /proc/device-tree/model 2>/dev/null || true)
for q in "$DIR"/quirks/*.sh; do
    [ -f "$q" ] || continue
    # shellcheck source=/dev/null
    . "$q"
done

mountpoint -q /sys/kernel/config 2>/dev/null || mount -t configfs none /sys/kernel/config
for m in libcomposite usb_f_hid usb_f_fs; do modprobe "$m" 2>/dev/null || true; done
UDC=$(ls /sys/class/udc/ 2>/dev/null | head -1)
[ -n "$UDC" ] || { echo "no UDC: is USB device (OTG/peripheral) mode enabled?" >&2; exit 1; }
G=$CONFIGFS/$GADGET_NAME

hid_report_pad() {     # HORI-style controller: 16 buttons, hat, 4 axes, vendor byte; 8-byte OUT
    printf '\x05\x01\x09\x05\xa1\x01\x15\x00\x25\x01\x35\x00\x45\x01\x75\x01\x95\x10\x05\x09\x19\x01\x29\x10\x81\x02\x05\x01\x25\x07\x46\x3b\x01\x75\x04\x95\x01\x65\x14\x09\x39\x81\x42\x65\x00\x95\x01\x81\x01\x26\xff\x00\x46\xff\x00\x09\x30\x09\x31\x09\x32\x09\x35\x75\x08\x95\x04\x81\x02\x06\x00\xff\x09\x20\x95\x01\x81\x02\x0a\x21\x26\x95\x08\x91\x02\xc0'
}
hid_report_kbd() {     # boot keyboard
    printf '\x05\x01\x09\x06\xa1\x01\x75\x01\x95\x08\x05\x07\x19\xe0\x29\xe7\x15\x00\x25\x01\x81\x02\x95\x01\x75\x08\x81\x01\x95\x05\x75\x01\x05\x08\x19\x01\x29\x05\x91\x02\x95\x01\x75\x03\x91\x01\x95\x06\x75\x08\x15\x00\x25\x65\x05\x07\x19\x00\x29\x65\x81\x00\xc0'
}
hid_report_mouse() {   # relative mouse: 5 buttons, X, Y, wheel
    printf '\x05\x01\x09\x02\xa1\x01\x09\x01\xa1\x00\x05\x09\x19\x01\x29\x05\x15\x00\x25\x01\x95\x05\x75\x01\x81\x02\x95\x01\x75\x03\x81\x01\x05\x01\x09\x30\x09\x31\x09\x38\x15\x81\x25\x7f\x75\x08\x95\x03\x81\x06\xc0\xc0'
}

hid_function() {       # name protocol subclass report_length no_out_endpoint descriptor-function
    local f=$G/functions/hid.$1
    mkdir -p "$f"
    echo "$2" > "$f/protocol"; echo "$3" > "$f/subclass"; echo "$4" > "$f/report_length"
    if [ -f "$f/no_out_endpoint" ]; then echo "$5" > "$f/no_out_endpoint"; fi
    # configfs takes each write() as the whole descriptor, and printf flushes at a 0x0A byte
    # (the controller descriptor has one): build it in a file, then copy it in one write.
    local tmp
    tmp=$(mktemp)
    "$6" > "$tmp"
    cat "$tmp" > "$f/report_desc"
    rm -f "$tmp"
}

wait_nodes() {
    for _ in $(seq 1 150); do
        local ok=1 n
        for n in "$@"; do [ -c "/dev/$n" ] || ok=0; done
        [ "$ok" = 1 ] && return 0
        sleep 0.02
    done
    return 1
}

release() {
    # Stop the Pro Controller daemon and unbind the UDC before anything else touches USB:
    # vendor helpers stopping a FunctionFS gadget otherwise wait forever in functionfs_unbind.
    # (nothing here may stop the script: after this point the USB side is down, and only going
    # on to build brings a controller back)
    local did=0 p i
    if p=$(pro_pid); then
        kill "$p" 2>/dev/null || true
        # until it is really gone (it holds the FunctionFS mount), at most 1.5 s
        i=0; while [ -d "/proc/$p" ] && [ "$i" -lt 30 ]; do sleep 0.05; i=$((i + 1)); done
        if [ -d "/proc/$p" ]; then kill -9 "$p" 2>/dev/null || true; sleep 0.1; fi
        did=1
    fi
    rm -f "$PRO_PID" "$ACTIVE_MODE" /run/ns2bench-host
    # one UDC can be owned by one gadget: unbind whichever gadget holds it
    for u in "$CONFIGFS"/*/UDC; do
        if [ -f "$u" ] && [ "$(cat "$u" 2>/dev/null)" = "$UDC" ]; then echo "" > "$u" 2>/dev/null || true; did=1; fi
    done
    # (settle only when something was let go of: at boot nothing is, and this was 0.6 s of waiting)
    if [ "$did" = 1 ]; then sleep 0.3; fi
}

teardown() {
    release
    mkdir -p "$G/configs/b.1/strings/0x409" "$G/strings/0x409"
    for link in "$G"/configs/b.1/f*; do if [ -L "$link" ]; then rm -f "$link"; fi; done
    # stale HID functions anywhere would keep their /dev/hidgN numbers: release them so ours are 0,1,2
    for link in "$CONFIGFS"/*/configs/*/*; do
        if [ -L "$link" ]; then case "$(readlink "$link")" in */functions/hid.*) rm -f "$link" ;; esac; fi
    done
    for f in "$CONFIGFS"/*/functions/hid.*; do if [ -d "$f" ]; then rmdir "$f" || echo "could not remove $f (still in use?)" >&2; fi; done
    if mountpoint -q "$FFS"; then umount "$FFS" 2>/dev/null || { sleep 0.3; umount "$FFS" || echo "could not unmount $FFS" >&2; }; fi
    if [ -d "$G/functions/ffs.pro2" ]; then rmdir "$G/functions/ffs.pro2" || echo "could not remove the pro2 function" >&2; fi
    if [ -f "$SPEED_SAVE" ] && [ -f "$G/max_speed" ]; then cat "$SPEED_SAVE" > "$G/max_speed" || true; fi
    if [ -f "$G/os_desc/use" ]; then echo 0 > "$G/os_desc/use"; fi
}

device() {             # vid pid bcdDevice class subclass protocol manufacturer product
    echo 0x0200 > "$G/bcdUSB"; echo 0x40 > "$G/bMaxPacketSize0"
    echo "$1" > "$G/idVendor"; echo "$2" > "$G/idProduct"; echo "$3" > "$G/bcdDevice"
    echo "$4" > "$G/bDeviceClass"; echo "$5" > "$G/bDeviceSubClass"; echo "$6" > "$G/bDeviceProtocol"
    echo "$7" > "$G/strings/0x409/manufacturer"; echo "$8" > "$G/strings/0x409/product"
}

# Bind the gadget to the UDC and check it took (EBUSY when another gadget still holds it: at boot
# a vendor USB helper may still hold it for a moment, so free it and try again for a few seconds).
# Callers sit in `build_x || fallback` lists, where set -e is off: every step returns its status.
bind() {
    for i in $(seq 1 10); do
        if echo "$UDC" > "$G/UDC" 2>/dev/null && [ "$(cat "$G/UDC" 2>/dev/null)" = "$UDC" ]; then
            [ "$i" = 1 ] || echo "bound on try $i"
            return 0
        fi
        for u in "$CONFIGFS"/*/UDC; do
            if [ "$u" != "$G/UDC" ] && [ -f "$u" ] && [ "$(cat "$u" 2>/dev/null)" = "$UDC" ]; then
                echo "the UDC is held by $(dirname "$u"): freeing it"; echo "" > "$u" || true
            fi
        done
        sleep 0.5
    done
    return 1
}

# Our Pro Controller daemon's pid from the pidfile, only if that process really is it (a pid in
# /run outlives a crash and can be reused by anything, an SSH session included).
pro_pid() {
    p=$(cat "$PRO_PID" 2>/dev/null) || return 1
    [ -n "$p" ] && [ -r "/proc/$p/cmdline" ] && tr '\000' ' ' < "/proc/$p/cmdline" | grep -q "ns2bench-board pro2" && echo "$p"
}

hid_report_pro1() {    # Switch 1 Pro Controller (057e:2009): reports 0x30 in, 0x21/0x81 replies, 0x01/0x10/0x80/0x82 out
    printf '\x05\x01\x15\x00\x09\x04\xa1\x01\x85\x30\x05\x01\x05\x09\x19\x01\x29\x0a\x15\x00\x25\x01\x75\x01\x95\x0a\x55\x00\x65\x00\x81\x02\x05\x09\x19\x0b\x29\x0e\x15\x00\x25\x01\x75\x01\x95\x04\x81\x02\x75\x01\x95\x02\x81\x03\x0b\x01\x00\x01\x00\xa1\x00\x0b\x30\x00\x01\x00\x0b\x31\x00\x01\x00\x0b\x32\x00\x01\x00\x0b\x35\x00\x01\x00\x15\x00\x27\xff\xff\x00\x00\x75\x10\x95\x04\x81\x02\xc0\x0b\x39\x00\x01\x00\x15\x00\x25\x07\x35\x00\x46\x3b\x01\x65\x14\x75\x04\x95\x01\x81\x02\x05\x09\x19\x0f\x29\x12\x15\x00\x25\x01\x75\x01\x95\x04\x81\x02\x75\x08\x95\x34\x81\x03\x06\x00\xff\x85\x21\x09\x01\x75\x08\x95\x3f\x81\x03\x85\x81\x09\x02\x75\x08\x95\x3f\x81\x03\x85\x01\x09\x03\x75\x08\x95\x3f\x91\x83\x85\x10\x09\x04\x75\x08\x95\x3f\x91\x83\x85\x80\x09\x05\x75\x08\x95\x3f\x91\x83\x85\x82\x09\x06\x75\x08\x95\x3f\x91\x83\xc0'
}

# Remote wakeup (the controller may wake a sleeping console), only while this file exists: an
# experiment, undone by deleting it (and switching mode or rebooting).
WAKE_BIT=0
[ -e /etc/ns2bench-remote-wakeup ] && WAKE_BIT=32
attrs() { printf '0x%02X' $(( $1 | WAKE_BIT )); }

# Switch 1 Pro Controller over USB (the bridge speaks its protocol on hidg0): works on a Switch 1,
# and a Switch 2 also takes the original Pro Controller.
build_pro1() {
    device 0x057E 0x2009 0x0200 0x00 0x00 0x00 "Nintendo Co., Ltd." "Pro Controller"
    echo "000000000001" > "$G/strings/0x409/serialnumber"
    echo "$(attrs 160)" > "$G/configs/b.1/bmAttributes"; echo 500 > "$G/configs/b.1/MaxPower"
    echo "NS2Bench" > "$G/configs/b.1/strings/0x409/configuration"
    if [ -f "$G/max_speed" ]; then
        [ -f "$SPEED_SAVE" ] || cat "$G/max_speed" > "$SPEED_SAVE"
        echo full-speed > "$G/max_speed"             # the real controller is a full-speed device
    fi
    hid_function usb0 0 0 64 0 hid_report_pro1
    ln -s "$G/functions/hid.usb0" "$G/configs/b.1/f1" || return 1
    bind || { echo "pro1: could not bind the gadget to $UDC" >&2; return 1; }
    wait_nodes hidg0 || { echo "pro1: hidg0 did not appear" >&2; return 1; }
}

build_hori() {         # with_mouse(0|1)
    device 0x0F0D 0x0092 0x0100 0x00 0x00 0x00 "PICO SWITCH PRO." "J24 CONTROLLER"
    echo "$(attrs 128)" > "$G/configs/b.1/bmAttributes"; echo 500 > "$G/configs/b.1/MaxPower"
    echo "NS2Bench" > "$G/configs/b.1/strings/0x409/configuration"
    hid_function usb0 0 0 64 0 hid_report_pad
    # keyboard LED SET_REPORT arrives on ep0; an OUT endpoint here made f_hid stall it
    hid_function usb1 1 1 8 1 hid_report_kbd
    ln -s "$G/functions/hid.usb0" "$G/configs/b.1/f1" || return 1
    ln -s "$G/functions/hid.usb1" "$G/configs/b.1/f2" || return 1
    local nodes="hidg0 hidg1"
    if [ "$1" = 1 ]; then
        hid_function usb2 2 1 4 1 hid_report_mouse
        ln -s "$G/functions/hid.usb2" "$G/configs/b.1/f3" || return 1
        nodes="$nodes hidg2"
    fi
    bind || { echo "could not bind the gadget to $UDC" >&2; return 1; }
    # shellcheck disable=SC2086
    wait_nodes $nodes || { echo "HID nodes did not appear: $nodes" >&2; return 1; }
}

build_pro() {
    device 0x057E 0x2069 0x0200 0xEF 0x02 0x01 "Nintendo" "Switch 2 Pro Controller"
    echo "00" > "$G/strings/0x409/serialnumber"
    echo "$(attrs 192)" > "$G/configs/b.1/bmAttributes"; echo 500 > "$G/configs/b.1/MaxPower"
    echo "Config_0" > "$G/configs/b.1/strings/0x409/configuration"
    if [ -f "$G/max_speed" ]; then
        [ -f "$SPEED_SAVE" ] || cat "$G/max_speed" > "$SPEED_SAVE"
        echo full-speed > "$G/max_speed"             # the real controller is a full-speed device
    fi
    mkdir -p "$G/functions/ffs.pro2" "$FFS"
    mountpoint -q "$FFS" || mount -t functionfs pro2 "$FFS" || return 1
    : > /tmp/ns2bench-pro2.log
    "$DIR/ns2bench-board" pro2 "$FFS" >> /tmp/ns2bench-pro2.log 2>&1 &
    echo $! > "$PRO_PID"
    for _ in $(seq 1 250); do [ -e "$FFS/ep4" ] && break; sleep 0.02; done
    [ -e "$FFS/ep4" ] || { echo "pro: FunctionFS rejected the descriptors" >&2; tail -5 /tmp/ns2bench-pro2.log >&2; return 1; }
    ln -s "$G/functions/ffs.pro2" "$G/configs/b.1/f1" || return 1
    bind || { echo "pro: could not bind the gadget to $UDC" >&2; return 1; }
    # Ready. The console starts the controller handshake by itself (now, or when it wakes); nothing
    # here waits for it, so the Bridge starts at once (it used to wait up to 6 s for the handshake).
    return 0
}

if [ "$mode" = status ]; then
    printf 'board=%s gadget=%s mode_file=%s\n' "${MODEL:-unknown}" "$GADGET_NAME" "$(cat "$MODE_FILE" 2>/dev/null || echo unset)"
    printf 'product=%s vid:pid=%s:%s\n' "$(cat "$G/strings/0x409/product" 2>/dev/null || echo ?)" \
        "$(cat "$G/idVendor" 2>/dev/null)" "$(cat "$G/idProduct" 2>/dev/null)"
    printf 'functions='; { ls "$G/configs/b.1" 2>/dev/null | grep '^f' || true; } | tr '\n' ' '; echo
    printf 'udc_state=%s speed=%s\n' "$(cat /sys/class/udc/*/state 2>/dev/null)" "$(cat /sys/class/udc/*/current_speed 2>/dev/null)"
    { ls /dev/hidg* 2>/dev/null || true; } | tr '\n' ' '; echo
    if [ -f "$PRO_PID" ]; then printf 'pro2_daemon=%s\n' "$(kill -0 "$(cat "$PRO_PID")" 2>/dev/null && echo running || echo dead)"; fi
    exit 0
fi

case "$mode" in standard|hori|pro|pro1) ;; *) echo "usage: gadget.sh [standard|hori|pro|pro1|status]" >&2; exit 2 ;; esac

# The early start (S45) may still be building when boot runs this again: wait for it (at most
# 30 s) rather than build at the same time (two builds at once fail, and a failed start switches
# controller mode off).
if [ -z "${1:-}" ] && [ "${NS2BENCH_EARLY:-}" != 1 ] && [ -e /run/ns2bench-early-running ]; then
    i=0; while [ -e /run/ns2bench-early-running ] && [ "$i" -lt 150 ]; do sleep 0.2; i=$((i + 1)); done
fi

# Boot runs this twice (early, and again at the end of boot): the second time, with the gadget
# already built and bound, leave it alone (a rebuild would make the console reconnect). Whatever
# mode is running counts, not only the saved one: the bench may have switched it in between.
already_up() {
    [ -z "${1:-}" ] && [ -e /run/ns2bench-early-done ] && [ "$(cat "$G/UDC" 2>/dev/null)" = "$UDC" ] || return 1
    case "$(cat "$ACTIVE_MODE" 2>/dev/null)" in standard|hori|pro|pro1) return 0 ;; esac
    return 1
}
if already_up "${1:-}"; then echo "NS2Bench: $(cat "$ACTIVE_MODE") already set up at boot"; exit 0; fi
lock
# (someone else may have built it while this run waited for its turn)
if already_up "${1:-}"; then echo "NS2Bench: $(cat "$ACTIVE_MODE") already set up at boot"; exit 0; fi

echo "== $(cut -d' ' -f1 /proc/uptime)s: gadget.sh ${1:-$mode} (boot default $(cat "$MODE_FILE" 2>/dev/null || echo unset))"

[ -x "$DIR/ns2bench-board" ] || { echo "missing ns2bench-board; install the static Rust NS2Bench Bridge daemon before changing USB mode" >&2; exit 1; }
release
quirk_before
mkdir -p "$G"
teardown
case "$mode" in
    standard) build_hori 1 || { echo "standard failed; trying controller + keyboard" >&2; teardown; build_hori 0; } ;;
    hori)     build_hori 0 ;;
    pro)      build_pro || { echo "pro failed; falling back to standard for now (pro stays the saved choice)" >&2; teardown; build_hori 1; mode=standard; } ;;
    pro1)     build_pro1 || { echo "pro1 failed; falling back to standard for now (pro1 stays the saved choice)" >&2; teardown; build_hori 1; mode=standard; } ;;
esac
quirk_after
echo "$mode" > "$ACTIVE_MODE"
[ -n "${1:-}" ] || history "gadget.sh boot run: $mode ready (udc $(cat /sys/class/udc/*/state 2>/dev/null))"
# An explicit mode becomes the boot default, but only once it was actually reached: a fallback
# must not quietly replace the person's choice.
# (NS2BENCH_NO_SAVE=1: a quick look, e.g. the bench finding which console this is; boot stays as it was)
if [ -n "${1:-}" ] && [ "$mode" = "$1" ] && [ "${NS2BENCH_NO_SAVE:-}" != 1 ]; then save_file "$MODE_FILE" "$mode"; fi
echo "NS2Bench: $mode ready ($(cat /sys/class/udc/*/state 2>/dev/null), $(cat /sys/class/udc/*/current_speed 2>/dev/null))"
