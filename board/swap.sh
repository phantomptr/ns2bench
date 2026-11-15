#!/bin/sh
# Turn on swap from an SD-card partition, if there is one: an out-of-memory safety net for boards
# with little RAM (the Luckfox has ~120 MB). Safe to run repeatedly; never formats anything.
# Candidates: $NS2BENCH_SWAP_DEVICE, a partition labelled CIVSWAP/SWAP, then the SD card partitions.
set -eu

if grep -q '^/dev/' /proc/swaps 2>/dev/null; then
    echo "swap: already active"; grep '^/dev/' /proc/swaps; exit 0
fi
for path in ${NS2BENCH_SWAP_DEVICE:-} /dev/disk/by-label/CIVSWAP /dev/disk/by-label/SWAP \
        /dev/disk/by-partlabel/CIVSWAP /dev/disk/by-partlabel/SWAP \
        /dev/mmcblk1p1 /dev/mmcblk1p2 /dev/mmcblk1p3 /dev/mmcblk1p4; do
    [ -b "$path" ] || continue
    # this buildroot's blkid reports no TYPE for a valid swap partition: only skip a partition
    # that blkid positively identifies as something else
    if command -v blkid >/dev/null 2>&1; then
        type=$(blkid -o value -s TYPE "$path" 2>/dev/null || true)
        [ -z "$type" ] || [ "$type" = swap ] || continue
    fi
    if swapon "$path" 2>/dev/null; then
        echo "swap: enabled $path"; exit 0
    fi
done
echo "swap: no SD swap partition found"
