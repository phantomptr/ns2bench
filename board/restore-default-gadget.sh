#!/bin/bash
# Recovery: give the board its vendor USB gadget back (undo ns2bench's console-facing gadget).
#   restore-default-gadget.sh [--from-job]
# On Luckfox: restores RNDIS+ADB via S50usbdevice, so the board is reachable over USB/ADB again.
# On other boards: unbinds the gadget so the board returns to its default state on next boot.
set -euo pipefail

# Run from an `adb shell`, stopping adbd below would end this script halfway (adbd is its parent).
# Then carry on detached, and say where the log is.
under_adbd() {
    p=$$
    while [ "${p:-1}" -gt 1 ]; do
        [ "$(cat "/proc/$p/comm" 2>/dev/null)" = adbd ] && return 0
        p=$(awk '{print $4}' "/proc/$p/stat" 2>/dev/null)
    done
    return 1
}
if [ -z "${NS2BENCH_DETACHED:-}" ] && command -v setsid >/dev/null 2>&1 && under_adbd; then
    NS2BENCH_DETACHED=1 setsid "$0" "$@" < /dev/null > /tmp/ns2bench-restore.log 2>&1 &
    echo "Restoring in the background (this ADB session will drop). Log: /tmp/ns2bench-restore.log"
    exit 0
fi

echo "Restoring default USB gadget..."

FROM_JOB=0
case "${1:-}" in
    "") ;;
    --from-job) FROM_JOB=1 ;;
    *) echo "Usage: $0 [--from-job]" >&2; exit 2 ;;
esac

# Stop the input daemon (it holds the HID devices) unless a mode switch is driving this
if [ "$FROM_JOB" = 0 ] && [ -x /opt/ns2bench/ns2bench-hidd.sh ]; then
    /opt/ns2bench/ns2bench-hidd.sh stop || true
fi

unbind_configfs_gadgets() {
    CONFIGFS=/sys/kernel/config/usb_gadget
    for gadget_dir in "$CONFIGFS"/*/; do
        [ -d "$gadget_dir" ] || continue
        if [ -f "$gadget_dir/UDC" ] && [ -n "$(cat "$gadget_dir/UDC" 2>/dev/null)" ]; then
            echo "" > "$gadget_dir/UDC" 2>/dev/null || true
        fi
    done
}

# The gadget quirks stop udev after creating the HID nodes. Do not run S10udev start
# here: that script triggers every subsystem, queues a full device settlement,
# and can block for its 30-second timeout. A single fresh udevd receives the
# gadget events produced below without replaying the entire boot hotplug set.
# (By exact process name, never a pattern: only udevd itself matches.)
for pid in $(pidof udevd 2>/dev/null || true); do kill "$pid" 2>/dev/null || true; done
if [ -x /sbin/udevd ]; then
    /sbin/udevd -d || true
fi

# The gadget quirks may have lowered the console log level while the console
# repeatedly reconfigured the gadget. Restore the saved value.
for saved in /run/ns2bench.printk; do
    [ -f "$saved" ] || continue
    saved_printk=$(cat "$saved" 2>/dev/null || true)
    if [ -n "$saved_printk" ]; then
        echo "$saved_printk" > /proc/sys/kernel/printk 2>/dev/null || true
    fi
    rm -f "$saved"
done

# A UDC can own only one gadget. Release ours before asking the vendor
# init script to recreate its RNDIS+ADB gadget.
if [ -x /etc/init.d/S50usbdevice ]; then
    unbind_configfs_gadgets
    # S50's restart path writes to a nonexistent per-config UDC attribute and
    # leaves stale HID links in the verified Luckfox layout. Do the unbind and
    # link cleanup explicitly, then call its normal start path.
    for link in /sys/kernel/config/usb_gadget/*/configs/*/*; do
        [ -L "$link" ] && rm -f "$link"
    done
    for process in usbdevice adbd; do
        for pid in $(pidof "$process" 2>/dev/null || true); do
            kill "$pid" 2>/dev/null || true
        done
    done
    for functionfs in /dev/usb-ffs/adb /dev/usb-ffs/ntb; do
        if mountpoint -q "$functionfs" 2>/dev/null; then
            umount "$functionfs" 2>/dev/null || true
        fi
        rmdir "$functionfs" 2>/dev/null || true
    done
    # The stock init script expects this file to select functions. Preserve an
    # existing profile, but add the normal RNDIS+ADB profile if the quirks
    # left only their guard marker.
    touch /tmp/.usb_config
    if ! grep -Eq '^usb_[a-z0-9_]+_en$' /tmp/.usb_config; then
        printf 'usb_rndis_en\nusb_adb_en\n' >> /tmp/.usb_config
    fi
    # Keep the vendor udev helper from restarting S50 concurrently on the
    # DISCONNECTED uevents produced by the unbind/rebind below. Removing this
    # guard too early causes its helper to call S50 without USB_CONFIG_FILE,
    # erase the configuration string, and reset usb0 to the vendor address.
    if ! grep -qx 'USB_FORCE_CHANGED' /tmp/.usb_config; then
        echo USB_FORCE_CHANGED >> /tmp/.usb_config
    fi
    USB_CONFIG_FILE=/tmp/.usb_config /etc/init.d/S50usbdevice start
    sleep 1
    sed -i '/^USB_FORCE_CHANGED$/d' /tmp/.usb_config 2>/dev/null || true
    echo "Default gadget restored (Luckfox RNDIS+ADB)."
else
    unbind_configfs_gadgets
    echo "NS2Bench gadget disabled. Reboot to restore defaults."
fi
