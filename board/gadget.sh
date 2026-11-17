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
SPEED_SAVE=/run/ns2bench-max-speed
mode=${1:-$(cat "$MODE_FILE" 2>/dev/null || echo standard)}

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
    for _ in $(seq 1 30); do
        local ok=1 n
        for n in "$@"; do [ -c "/dev/$n" ] || ok=0; done
        [ "$ok" = 1 ] && return 0
        sleep 0.1
    done
    return 1
}

release() {
    # Stop the Pro Controller daemon and unbind the UDC before anything else touches USB:
    # vendor helpers stopping a FunctionFS gadget otherwise wait forever in functionfs_unbind.
    if p=$(pro_pid); then kill "$p"; sleep 0.5; fi
    rm -f "$PRO_PID"
    # one UDC can be owned by one gadget: unbind whichever gadget holds it
    for u in "$CONFIGFS"/*/UDC; do
        if [ -f "$u" ] && [ "$(cat "$u" 2>/dev/null)" = "$UDC" ]; then echo "" > "$u"; fi
    done
    sleep 0.3
}

teardown() {
    release
    mkdir -p "$G/configs/b.1/strings/0x409" "$G/strings/0x409"
    for link in "$G"/configs/b.1/f*; do if [ -L "$link" ]; then rm -f "$link"; fi; done
    # stale HID functions anywhere would keep their /dev/hidgN numbers: release them so ours are 0,1,2
    for link in "$CONFIGFS"/*/configs/*/*; do
        if [ -L "$link" ]; then case "$(readlink "$link")" in */functions/hid.*) rm -f "$link" ;; esac; fi
    done
    for f in "$CONFIGFS"/*/functions/hid.*; do if [ -d "$f" ]; then rmdir "$f"; fi; done
    if mountpoint -q "$FFS"; then umount "$FFS"; fi
    if [ -d "$G/functions/ffs.pro2" ]; then rmdir "$G/functions/ffs.pro2"; fi
    if [ -f "$SPEED_SAVE" ] && [ -f "$G/max_speed" ]; then cat "$SPEED_SAVE" > "$G/max_speed" || true; fi
    if [ -f "$G/os_desc/use" ]; then echo 0 > "$G/os_desc/use"; fi
}

device() {             # vid pid bcdDevice class subclass protocol manufacturer product
    echo 0x0200 > "$G/bcdUSB"; echo 0x40 > "$G/bMaxPacketSize0"
    echo "$1" > "$G/idVendor"; echo "$2" > "$G/idProduct"; echo "$3" > "$G/bcdDevice"
    echo "$4" > "$G/bDeviceClass"; echo "$5" > "$G/bDeviceSubClass"; echo "$6" > "$G/bDeviceProtocol"
    echo "$7" > "$G/strings/0x409/manufacturer"; echo "$8" > "$G/strings/0x409/product"
}

# Bind the gadget to the UDC and check it took (EBUSY when another gadget still holds it).
# Callers sit in `build_x || fallback` lists, where set -e is off: every step returns its status.
bind() { echo "$UDC" > "$G/UDC" && [ "$(cat "$G/UDC" 2>/dev/null)" = "$UDC" ]; }

# Our Pro Controller daemon's pid from the pidfile, only if that process really is it (a pid in
# /run outlives a crash and can be reused by anything, an SSH session included).
pro_pid() {
    p=$(cat "$PRO_PID" 2>/dev/null) || return 1
    [ -n "$p" ] && [ -r "/proc/$p/cmdline" ] && tr '\000' ' ' < "/proc/$p/cmdline" | grep -q "ns2bench-board pro2" && echo "$p"
}

build_hori() {         # with_mouse(0|1)
    device 0x0F0D 0x0092 0x0100 0x00 0x00 0x00 "PICO SWITCH PRO." "J24 CONTROLLER"
    echo 0x80 > "$G/configs/b.1/bmAttributes"; echo 500 > "$G/configs/b.1/MaxPower"
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
    echo 0xC0 > "$G/configs/b.1/bmAttributes"; echo 500 > "$G/configs/b.1/MaxPower"
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
    for _ in $(seq 1 50); do [ -e "$FFS/ep4" ] && break; sleep 0.1; done
    [ -e "$FFS/ep4" ] || { echo "pro: FunctionFS rejected the descriptors" >&2; tail -5 /tmp/ns2bench-pro2.log >&2; return 1; }
    ln -s "$G/functions/ffs.pro2" "$G/configs/b.1/f1" || return 1
    bind || { echo "pro: could not bind the gadget to $UDC" >&2; return 1; }
    for _ in $(seq 1 60); do grep -q 'console initialised USB' /tmp/ns2bench-pro2.log && return 0; sleep 0.1; done
    # The controller is up; the console just hasn't talked to it yet (asleep, off, slow to wake).
    # Stay a Pro Controller: it starts the handshake when it wakes.
    echo "pro: ready, waiting for the console (asleep or off?) to start the controller handshake" >&2
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

case "$mode" in standard|hori|pro) ;; *) echo "usage: gadget.sh [standard|hori|pro|status]" >&2; exit 2 ;; esac

[ -x "$DIR/ns2bench-board" ] || { echo "missing ns2bench-board; install the static Rust NS2Bench Bridge daemon before changing USB mode" >&2; exit 1; }
release
quirk_before
mkdir -p "$G"
teardown
case "$mode" in
    standard) build_hori 1 || { echo "standard failed; trying controller + keyboard" >&2; teardown; build_hori 0; } ;;
    hori)     build_hori 0 ;;
    pro)      build_pro || { echo "pro failed; falling back to standard for now (pro stays the saved choice)" >&2; teardown; build_hori 1; mode=standard; } ;;
esac
quirk_after
# An explicit mode becomes the boot default, but only once it was actually reached: a fallback
# must not quietly replace the person's choice.
if [ -n "${1:-}" ] && [ "$mode" = "$1" ]; then echo "$mode" > "$MODE_FILE"; fi
echo "NS2Bench: $mode ready ($(cat /sys/class/udc/*/state 2>/dev/null), $(cat /sys/class/udc/*/current_speed 2>/dev/null))"
