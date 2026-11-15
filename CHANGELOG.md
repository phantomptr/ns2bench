# Changelog

What's new in NS2Bench, written for humans.

---

## Unreleased

- **Connect an agent in one click**, like ps5upload. Settings → API & MCP shows the MCP address and a card each for Claude Code, Codex and OpenCode: whether it has NS2Bench, an Add or Remove button, and the command to run yourself. Agents now connect to the running app by address (`http://127.0.0.1:7630/mcp`), with nothing to install or point at.
- Settings → API & MCP is split into Connect an agent, Agent tools and API, with copy buttons on every command.

## 0.0.6

- `ns2bench serve` now prints a link ending in `?ui=…`. Open that link to drive from your browser. A page opened without it can watch but not drive, so other programs on the computer can no longer pose as you. The desktop app is unchanged.
- Fixed: a serial input device (Pico, CH9329) that was plugged in but never answered slowly used up the engine's threads on macOS and Linux. The port is also set up more reliably on macOS.
- Keyboard & mouse mode: Ctrl+Alt+U gives the keyboard back to the app, so you can leave the picture without a mouse.
- Joy-Con tooltips appear in the right place at every Text size.
- More of the interface is translated: Joy-Con button names, input warnings, update status and a few short messages.

## 0.0.5

- New round buttons in the top bar, like ps5upload: reconnect the picture, sound and status; open Updates (a dot shows when one is ready); a bell that lists recent notifications with an unread count; and the light/dark switch.
- Your choices are remembered like in ps5upload: theme, text size, language, layout and notification settings are saved in `~/.ns2bench/ui.json`, so they are the same in the app and in a browser and survive a cleared browser.
- The version is shown under the NS2Bench name in the top bar.
- A **Thank you** button in the top bar and in Help, for anyone who wants to support NS2Bench.
- The on-screen ZL and ZR now wrap around the Joy-Con's corner with no gap, and L and R are longer.

## 0.0.4

- Fixed: with **Block agents** on, the app's own window was refused too. Switching the USB mode, changing video or input settings, saving a clip and the screen-text log now work for the person at the window while agents stay blocked.
- Fixed inputs that could stay held:
  - a keyboard key tapped very briefly
  - a key pressed together with Command on macOS
  - a mouse button held by an agent when Block agents was turned on, or held when the window closed
  - an on-screen button that was right-clicked
- Fixed: a hold or stick move sent while the Bridge was offline no longer takes effect later when it reconnects.
- Security: an agent can no longer change the Bridge address or token file, which could have sent the Bridge token to another computer. Only the person at the app window can.
- A browser tab opened next to the desktop app now says it can watch but not drive, instead of silently ignoring input.
- Fullscreen fits the screen at every Text size.
- Logs catch up after a reconnect, and the live picture keeps trying to reconnect after the engine restarts.
- Tests stop when Block agents is turned on, are limited to 500 steps and save one failure clip per run.
- Windows: installing the command line tool keeps entries like `%USERPROFILE%` in your PATH as they were, and no console windows flash up.
- Linux: a `.deb` install is told where to get the new `.deb`, instead of downloading an update it can't install. The network helper also refuses to run when any single interface has forwarding on.
- "Install command line tool" no longer replaces an `ns2bench` that something else put there.
- A Bridge board install that fails to start puts the previous version back.
- MCP: `run_test` with a file path works, and `wait_for_screen` with a screenshot also says whether the screen changed.
- The website's download buttons always point at the latest published release.

## 0.0.3

- macOS remembers Camera and Microphone permission. The app now has a fixed signature, so it asks once instead of after every update. Updating from 0.0.2 asks one last time.
- New Settings → Install tab:
  - Install the `ns2bench` command line tool, which now ships inside the app.
  - Flash a Raspberry Pi Pico with the Bridge firmware.
  - Install or update the Bridge on a Linux board over SSH.
  - Install the offline network helper as a service on macOS or Linux.
- The on-screen Switch 2 looks like the real console: one device with a bezelled screen, Joy-Con attached with coloured rails, and shoulder buttons on the top edge.
- Settings has a sidebar, and Appearance works like ps5upload. It has theme cards, a Text size setting (70–140%) and Accessibility options: Motion, Reduce transparency, High contrast and a dyslexia-friendly font.
- Logs have levels: Everything, Info, Problems and Errors. Each level has an icon, and known problems now explain what happened and what to do. Logs are also saved to `~/.ns2bench/logs` (7 days). More things are logged: API calls, refused requests, the picture stopping or running slow, sound problems, and settings changes.
- Desktop notifications, like ps5upload: when the Bridge or the picture drops, when tests and clips finish, when an agent starts driving, and when an update is ready (Settings → Notifications).
- Agents and scripts can do more, in the API, MCP and the command line alike:
  - wait for the screen to change or settle
  - find text and where it is
  - take cropped screenshots
  - get a one-call health check with fixes
  - read events, clips, test reports and video info
  - filter logs by level
- Fixed: agents could switch the USB mode or video settings while Block agents was on. Probes now appear in the input log, and `type` no longer types at half speed.
- **Help** in the top bar: run a check of every part, read short answers to common problems, or save a bug report. The status pills now say what's wrong and what clicking them does.
- Security:
  - When the app runs its own engine, only its own window counts as the person at the bench. A script can no longer pose as the window to turn Block agents off.
  - Block agents now also stops agents from changing the input device, the console network, the screen log and clips. Typing stops mid-text when you turn it on.
  - Input token files must be `.token` files in `~/.ns2bench`, so other files can't be sent to the Bridge host.
  - On macOS, updates must be signed with NS2Bench's own certificate.
  - Weekly CodeQL and RustSec scans.
- Fixes:
  - Sound follows when you switch capture cards.
  - Clips are saved as MP4 when ffmpeg was installed with Homebrew and the app was opened from Finder.
  - The serial baud rate is now set on Windows.
  - The live view no longer smears after it falls behind.
  - The Bridge board no longer spins its CPU in Pro mode while its controller helper restarts.
  - The macOS network helper no longer writes an ever-growing output file.
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
