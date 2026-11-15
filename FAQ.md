# General

## What is NS2Bench?

A bench for a Nintendo Switch 2 on your desk. It shows the console's picture and sound on your computer and lets you, or an AI agent, press its buttons. You can record clips, read on-screen text, and run repeatable tests.

## Who is it for?

Developers testing software on real hardware, people automating repetitive checks, accessibility experiments, and anyone who wants an AI agent to operate a console while they watch.

## Does it modify my console?

No. NS2Bench only does what a TV and a wired controller do: it receives HDMI and presses buttons over USB. Nothing is installed on the console.

## Does the console go online?

No. NS2Bench never gives the console internet access. Capture and USB input need no network at all. The optional console network answers only the console's connectivity test and never forwards traffic. See [Offline console network](#offline-console-network).

## Is it free?

Yes. NS2Bench is free and open source under the MIT license. If it's useful, you can [support it](README.md#support).

## Is it affiliated with Nintendo?

No. NS2Bench is an independent project. Nintendo Switch is a trademark of Nintendo.

# Setup

## What do I need?

A capture card for video and sound (always), and one input device if you want to press buttons:

- a Linux board with a USB device port (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5),
- or a Raspberry Pi Pico plus a 3.3 V USB-to-UART adapter,
- or a CH9329 module (keyboard and mouse only).

The [setup guide](docs/setup/README.md) has a diagram and steps for each.

## Which input device should I pick?

If you want every button, including C, GL and GR, use a Linux board: its Pro mode shows the console a real Switch 2 Pro Controller. A Pico is the cheapest controller option (all buttons except C, GL and GR). A CH9329 gives only a keyboard and mouse. You can start with view only and add an input device later.

## Do I need the dock?

The console needs a way to output HDMI, which normally means the dock. The input device plugs into one of the dock's USB ports.

## Can I use it without an input device?

Yes. Choose **View only** in Settings → Devices. You can watch, listen, take screenshots, save clips and read on-screen text.

## Where do I set the capture card?

In the **Video** panel on the right of the main window. It lists only the formats your card offers.

## Where do I set the input device?

In **Settings → Devices**. Changes apply without restarting; the old connection releases every button first.

## How do I know it works?

The **video** pill in the top bar turns green with a frame rate, and the **Bridge** pill turns green when the console has accepted the input device. **Diagnostics** in the top bar copies a full status summary.

# Capture and sound

## Which capture cards work?

Any USB (UVC) HDMI capture card. 1080p60 is the goal. Tested: UGREEN 15389 (MS2130), at 1080p60 and 1440p30.

## Can I use OBS at the same time?

On macOS, yes: NS2Bench shares the card with OBS and never takes it away. On Windows and Linux, ffmpeg usually needs the card for itself, so close other apps that hold it.

## There's no picture.

Check the console is docked and on, the HDMI cable goes into the card's **input**, and the right card is selected in the Video panel. On macOS, allow **Camera** for NS2Bench in System Settings → Privacy & Security. Run **Diagnostics** for details.

## There's no sound.

Click **Sound off** in the top bar to turn sound on. On macOS, allow **Microphone** for NS2Bench. If you have two identical capture cards, NS2Bench pairs the sound with the picture by USB position.

## How much CPU does capture use?

On macOS, about 12% of one core at 1080p60 (hardware JPEG and H.264). On Linux, MJPEG cards are passed through without re-encoding.

# Input devices

## The Bridge pill stays red.

- **Linux board:** check the board is powered and reachable at the address in Settings → Devices, and that the token file matches the board's token.
- **Pico:** check the serial port and that the baud rate is 921600; TX and RX must cross (adapter TX → Pico GP1, adapter RX → Pico GP0) and GND must be connected.
- **CH9329:** match the module's baud rate (usually 9600).

Then check the input device is plugged into the console's USB port and the console is awake.

## Presses don't register.

The Switch 2 needs a press to last at least about 50 ms. NS2Bench holds every press for at least 75 ms. If the Bridge is green but nothing happens, the console may be asleep or showing a dimmed screen: the first press only wakes it.

## How do I get C, GL and GR?

GL and GR are the two back buttons on the Nintendo Switch 2 Pro Controller; Joy-Con 2 don't have them. To use them (and C), use a Linux board and choose **The console sees → Pro Controller** in the Driving panel. The console then sees a Switch 2 Pro Controller. Otherwise C works as a GameChat shortcut.

## Can I use my own gamepad?

Yes. Plug it into the computer; NS2Bench maps it by button position (the bottom face button is B).

## Can I type with a keyboard?

Choose **Your keyboard sends → Keyboard & mouse** in the Driving panel. The console sees a real USB keyboard. This needs a Linux board or a Pico in standard mode, or a CH9329.

## What happens when the console sleeps?

A board powered from the console's USB loses power and restarts when the console wakes. NS2Bench reconnects by itself. Presses made while the Bridge is offline are dropped, never replayed later.

# Offline console network

## Do I need a network?

No. Capture and USB input work without any network. The console network is only for tools that talk to the console over a LAN.

## How do I give the console a LAN without internet?

Connect the console's Ethernet (dock port or USB adapter) to a dedicated Ethernet adapter on your Mac or Linux computer and run `ns2bench-netd`. It hands out addresses and answers only the connectivity test. Or use a router, VLAN or Wi-Fi network with internet blocked. See the [setup guide](docs/setup/README.md#optional-offline-console-network).

## Can I turn internet blocking off?

No. It isn't a setting. NS2Bench never forwards, NATs or proxies console traffic and never asks an upstream DNS server.

## I'm on Windows.

The network service runs on macOS and Linux. On Windows, run it on a Linux board or PC that owns the console's Ethernet port and control it from the Network tab ([remote helper](docs/REFERENCE.md#remote-helper-windows-hosts)), or use a router with internet blocked.

# Agents and API

## Which agents work?

Any MCP client: Claude Code, Codex, opencode and others. Settings → API & MCP shows a ready-to-copy configuration.

## Can I stop an agent?

Yes. **Block agents (manual only)** in the Driving panel stops agent input immediately. Only you, from the NS2Bench window, can turn it off again.

## Is the API safe on my network?

The engine listens only on `127.0.0.1` by default. Listening on another address requires a token. Web pages from other sites can't send it commands.

## Where is the API documented?

In [docs/REFERENCE.md](docs/REFERENCE.md#agents-mcp-and-http-api).

# App and updates

## How do updates work?

The app checks GitHub releases daily and shows when an update is available. Every download is checked against the release's SHA-256 before installing. Automatic download and install are off by default; turn them on in Settings → Updates.

## macOS says the app is damaged or can't be checked.

The app isn't notarized yet. Run `xattr -dr com.apple.quarantine /Applications/NS2Bench.app` once, then open it.

## Windows says it protected my PC.

Choose **More info → Run anyway**. The installer isn't signed with a paid certificate yet.

## macOS asks for Camera or Microphone again after an update.

Until the app is notarized, each update counts as a new app to macOS, so it may ask again. Allow it once more; nothing else changes.

## How do I change the theme or language?

Settings → Appearance. The moon button in the top bar switches between light and dark.

## Where are my clips, screenshots and logs?

In `~/.ns2bench/` (clips, reports, logs). Nothing is uploaded anywhere.

# Troubleshooting

## How do I report a bug?

Click **Bundle** in the top bar to save a bug-report zip (logs, status, a screenshot; no tokens) and attach it to a [GitHub issue](https://github.com/phantomptr/ns2bench/issues/new/choose).

## What does the doctor do?

`ns2bench doctor` checks video, the console picture, OCR, the Bridge, USB state, calibration and network isolation, and says which part is broken and what to do. `--probe` also presses RIGHT and LEFT on the HOME menu to test input end to end.

## Where can I ask for help?

On [Discord](https://discord.gg/fzK3xddtrM).
