# Changelog

What's new in NS2Bench, written for humans.

---

## Unreleased

- macOS remembers Camera and Microphone permission. The app now has a fixed signature, so it asks once instead of after every update. Updating from 0.0.2 asks one last time.
- New Settings → Install tab:
  - Install the `ns2bench` command line tool, which now ships inside the app.
  - Flash a Raspberry Pi Pico with the Bridge firmware.
  - Install or update the Bridge on a Linux board over SSH.
  - Install the offline network helper as a service on macOS or Linux.
- The on-screen Switch 2 looks like the real console: one device with a bezelled screen, Joy-Con attached with coloured rails, and shoulder buttons on the top edge.
- Settings has a sidebar, and Appearance works like ps5upload. It has theme cards, a Text size setting (70–140%) and Accessibility options: Motion, Reduce transparency, High contrast and a dyslexia-friendly font.
- Windows on ARM: a native `win-arm64` installer and command line tool.

## 0.0.2

- **Languages:** the Network settings status (service state, interface, DHCP/DNS/connectivity
  switches, helper errors) is now translated in every language.
- **FAQ:** the answer about sleep now says, in every language, that the input board boots and
  NS2Bench reconnects by itself within about 30 seconds after you wake the console (tested).

## 0.0.1

The first public release. NS2Bench lets you see, hear and drive a Nintendo Switch 2 from
your computer, by hand or through AI agents, while the console stays offline.

- **See and hear:** live 1080p60 video and sound from an HDMI capture card, with
  screenshots, 30-second clips and on-screen text reading (OCR). On macOS it shares the
  card with OBS.
- **Drive:** keyboard, mouse or your own gamepad, sent through an input device on the
  console's USB port: a Linux USB-gadget board, a Raspberry Pi Pico, or a CH9329 module
  (keyboard and mouse only). A board can also act as a Switch 2 Pro Controller with C,
  GL and GR.
- **Agents:** an HTTP API and an MCP server with 23 tools. A manual-only switch blocks
  agent input whenever you want the controls to yourself.
- **Tests:** repeatable JSON tests with screenshots and a clip when a step fails,
  navigation routes, calibration and a doctor that says which part is broken.
- **Offline console network (optional):** local DHCP, a DNS that answers only the
  connectivity test, and no forwarding, NAT or proxy. Windows can control a Linux helper.
- **Desktop app:** for macOS, Windows and Linux, with verified updates, a getting-started
  checklist, a fullscreen toolbar, a one-click bug report, Light, Peach, Dark and OLED
  themes, and 11 interface languages.

Tested on hardware: macOS capture and a Luckfox Linux board. Not yet tested on hardware:
the Raspberry Pi Pico firmware, a real CH9329 module, and capture on Windows and Linux.
