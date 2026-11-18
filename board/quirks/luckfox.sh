# Luckfox Pico (RV1106) quirks for gadget.sh. Sourced only on Luckfox boards.
case "${MODEL:-}" in *Luckfox*|*luckfox*) ;; *) return 0 ;; esac

# Reuse the vendor's gadget directory: the enumeration path verified against the Switch 2.
GADGET_NAME=rockchip

quirk_before() {
    # The vendor Android-style USB helper reacts to every CONFIGURED uevent; on a console that
    # fired every few ms, relaunching /usr/bin/usbdevice until memory ran out. Mark the config as
    # forced, stop the helper and adbd before taking the UDC.
    grep -qx USB_FORCE_CHANGED /tmp/.usb_config 2>/dev/null || echo USB_FORCE_CHANGED >> /tmp/.usb_config
    if [ -x /etc/init.d/S50usbdevice ]; then /etc/init.d/S50usbdevice stop >/dev/null 2>&1 || true; fi
    for p in usbdevice adbd; do
        for pid in $(pidof "$p" 2>/dev/null || true); do kill "$pid" 2>/dev/null || true; done
    done
    # udev relaunches that helper on every CONFIGURED uevent, and the helper rebuilds this same
    # gadget: stop udev before the UDC is taken, not after (a Pro Controller start waits for the
    # console's handshake, long enough for the helper to take the gadget over at boot).
    # HID and FunctionFS nodes come from devtmpfs, so nothing here needs udev.
    if [ -x /etc/init.d/S10udev ]; then /etc/init.d/S10udev stop >/dev/null 2>&1 || true; fi
    # Rockchip logs CONFIGURED for HID SET_REPORT too; console logging of that costs CPU.
    [ -f /run/ns2bench.printk ] || cat /proc/sys/kernel/printk > /run/ns2bench.printk
    dmesg -n 1 2>/dev/null || true
}

quirk_after() {
    # HID nodes come from devtmpfs; stop udev's queue so repeated uevents spawn no vendor helpers.
    if [ -x /etc/init.d/S10udev ]; then /etc/init.d/S10udev stop >/dev/null 2>&1 || true; fi
}
