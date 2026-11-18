# Changelog

What's new in NS2Bench, written for humans.

---

## Unreleased

- **The Luckfox is ready much sooner after power-up:** usable as a Pro Controller about **5.4 s** after power-on (it was 13 s). Its controller set-up runs early in boot, the vendor's unused USB setup is skipped, and it starts in the mode the console it was last on needs, Switch 1 or Switch 2. Install the board update from **Settings → Install → Bridge board**.
- **Switching the controller's mode no longer drops the board's connection.** Only the console sees the controller change, about 1 s on the board. There's no "Board ✗", no reconnecting and no "which console?" question afterwards.
- **NS2Bench notices the board coming back sooner** after a console wakes up (it checks every half second).
- **Fewer "Bridge disconnected" notifications.** A console going to sleep turns the board off, which is normal, so that's only logged now. You're notified when the board stays away for 20 s while a console is on.
- **Mouse capture works in the Mac app,** like ps5upload's play mode. Click the picture (or Shift+click with controller buttons) and the mouse goes to the console, with a faint note on how to release it. Esc, leaving the window or hiding it always gives the cursor back.
- **Sleep is greyed out without HDMI-CEC,** and says how to set it up. Wake works through the controller.
- **The health check understands the shared home network.** It looks for each console on the network instead of warning about missing DHCP leases. The greyed-out DHCP switch explains why it's off.
- **Settings → Install** makes clear the Raspberry Pi Pico firmware isn't for a Luckfox, and how a Pico connects.
- **Thank you** moved to the far right of the top bar.

## 0.0.23

- **The shared controller follows the console you switch to.** Click a console and the controller (and the mode that console's type needs) goes with it. Move its cable to that console. If the board shows it's still plugged into the Switch 2, NS2Bench says so instead of switching. Turn it off, or leave a console out, in **Settings → Consoles**.
- **Console type, spelled out.** Each console's type says what the controller does there: a Switch 2 Pro Controller, or a wired controller + keyboard for a Switch 1. NS2Bench also notices when a console's network traffic says it's the other type, and offers to fix it.
- **Settings save themselves.** Picture & sound and every console's settings save the moment you change them, for that console, with no Apply or Save buttons.
- **Hide the console pictures:** "Hide pictures" turns the strip into small name chips, still one click to switch.
- **Finding the controller's console after a console wakes is faster:** the console that just woke up runs its connection test, and NS2Bench uses that instead of switching the board's mode to check. The quick check no longer becomes the board's start-up mode.
- **Fixed:**
  - Settings → Consoles listed no capture cards on Mac, only the saved one marked "not connected". Automatic card picks found no cards on Mac either.
  - Two consoles left on the same capture card now get sorted out when NS2Bench starts.
  - Thumbnails no longer go blank when the strip updates.
  - The Bridge restarting for a mode change, or because a console went to sleep, is no longer reported as an error.
  - Every console network lookup no longer fills the log.
- **Smoother:** less work on every screen refresh (the controller drawing only redraws when something changes, no blur on the top bar, the ❤️ stops after a few beats) and thumbnails refresh every 2 seconds, only for consoles with a picture.

## 0.0.22

- **NS2Bench finds which console the controller is plugged into by itself** (one Switch 2 and one other console). Only a Switch 2 starts the board as a Pro Controller, so NS2Bench checks for that, briefly switching the board's mode if needed. It never presses a button to find out. You can still pick the console yourself at any time. Needs the updated board software: **Settings → Install → Bridge board**.
- **Fixed: keys did nothing after the board restarted in Pro mode.** When Pro mode failed to start, the board quietly ran as the standard controller but still sent presses to the Pro Controller. It now reports and uses the mode that's really running. Pro mode is also more reliable at boot: the vendor's USB helper can no longer take over the board while it starts.
- **When keys are paused, the Controller card says so,** with a button for each console. Before, it showed all green while every key was being held back.
- **Fixed: the Picture & sound panel showed the first console's settings** after you switched to another console. Pressing Apply then tried to give the second console the first one's capture card.

- **Moving the controller between consoles is smoother:**
  - Switching the board's mode no longer makes NS2Bench ask "which console?" a second time.
  - If you say where the controller is while the board is still restarting, NS2Bench remembers it and sets the right mode when the board is back.

## 0.0.21

- **A new right panel that follows the console you watch:**
  - **Console:** its name and model, where the controller is, and Edit, Wake and Sleep.
  - **Controller:** shows each link (computer → board → console) and says what to do when one is down. For example: "the board has power but Switch 1 hasn't connected to it".
  - **Controller mode:** says which mode the console needs, with one click to switch when it's wrong. **Let go of everything** and **Run the check** are here too.
  - **Picture & sound:** sound, screenshot and "save last 30 s" next to the capture card. Resolution, FPS and quality are folded away.
  - **Network:** whether the console is protected, how many Nintendo names it was refused, when it last ran the connection test, and whether the helper is running.
- **A console needs only a name.** Its capture card, network address and colour can come later. Until it has a card, the page says so with a **Set up** button.
- **Edit a console from the main page:** the ✎ on its picture opens its settings.
- **A console's saved capture card is kept while unplugged.** Before, Settings showed "Pick automatically" for an offline card, and saving would forget it.
- **Colours are dots to click** (plus a custom colour). The old colour box showed as an empty bar.
- **Adding a console is easy to find:** the console strip under the picture now shows from the first console, with **+ Add console** next to it. Clicking it opens the form, ready to type the name.
- The network helper walks the computer's adapter list in a way GitHub's code scanning can check (no change in behaviour).

## 0.0.20

- **Several consoles, safer:**
  - When NS2Bench starts with several consoles, it asks which one the controller is plugged into, since the cable may have moved while it was closed.
  - Every action that can press a button (power, the health check, tests, sequences) only goes to the console the controller is on.
  - A test or sequence stops if the controller is moved while it runs.
  - The cable-move check no longer goes off when a console just sleeps and wakes, and notices when the board itself restarts.
  - Removing a console stops its sound and recording, so its capture card is free again.
  - Console names with accents work from agents.
  - Two consoles no longer share a capture-format fallback.
  - A damaged consoles file is kept aside instead of being overwritten.
- **Console strip and settings:**
  - The picture follows the console you watch, also when the controller moves.
  - Changing a console's capture card reconnects its picture.
  - One stuck card no longer freezes all the thumbnails.
  - A failed save in Settings → Consoles shows what was really saved.
- **Keyboard focus** on ZL and ZR is fully visible again.
- **Security fix:** the network helper reads the computer's network adapters more carefully (a macOS netmask record can be shorter than a full address; it was read past its end). Flagged by GitHub code scanning.

## 0.0.19

- Not published: it was stopped for one more review. Its security fix and everything that review found are in 0.0.20.

## 0.0.18

- **Several consoles, now as live pictures.** Under the console, each console has a small live picture with its name and colour, and 🎮 on the one the controller is plugged into. Click one to watch it big, or **+ Add console**.
- **New top buttons.** ZL, L, R and ZR are spread evenly along the top and reach halfway over the screen. Prefer them meeting in the middle? **Settings → Appearance → Top buttons → Full span**. Nothing covers the screen any more.
- **Setup guide: how input devices connect.** What a Linux board, a Pico and a CH9329 each need to reach the console and the computer, and why a Linux board like the Luckfox is the recommended one.

## 0.0.17

- **Several consoles on one bench.** Give each console a name, a model (Switch 2 or Switch 1), its own capture card and network address. All their pictures run at once.
  - A **console bar** above the picture switches between them. Screenshots, clips, screen reading and tests work on the console you're watching.
  - They can share one controller: 🎮 shows which console it's plugged into, and buttons only ever go there. Move its cable and click **I moved the controller to …**. If NS2Bench sees the cable move without being told, it pauses input and asks.
  - The board switches to the right controller mode for each model (Switch 2: Pro Controller; Switch 1: standard).
  - Set it up in **Settings → Consoles**. Agents name a console with `console` (MCP) or `?console=` (API), and `list_consoles` lists them. A single console works exactly as before.
- **Safer offline network.** NS2Bench can no longer hand out addresses on your home network, even if a settings file goes missing. The leak watch keeps checking while the network helper is stopped, and says so.
- **Signed downloads for the board, Pico and network helper**, like app updates.
- **Fixes:**
  - A mouse button no longer stays held after the controller disconnects mid-click.
  - With the app and a browser tab both open, one no longer lets go of buttons the other holds.
  - Windows: serial input no longer stutters, and flow-control settings left by other programs are cleared.
  - A hardware video encoder that doesn't work on this computer no longer stops the picture.
  - The × on messages works again while Settings is open.
  - The shared network setup picks the right network adapter by itself.
  - Pro Controller mode no longer reports success when the board couldn't set it up.
- The controller drawing's L/ZL and R/ZR buttons are wider and sit closer to the middle.

## 0.0.16

- **Keep consoles offline on your home Wi-Fi, with no extra hardware.** New in Settings → Network: **Shared home network**.
  - Pick your computer's network adapter from a list, enter the consoles' addresses, and NS2Bench shows exactly what to type on each console.
  - Each console's gateway is its own address, so nothing leaves your network, by name or by IP address.
  - NS2Bench answers only the connection test, so consoles say they're connected. Every other name is refused, Nintendo's included, and only the consoles you list get any answer.
  - **Leak watch:** if a console is on your network but stops using NS2Bench (for example after a reset put it back on automatic settings), NS2Bench warns you.
  - Step-by-step guide with a diagram: setup guide, section D.

## 0.0.15

- **Windows: NS2Bench closing by itself.** Some people saw NS2Bench find the capture card and then close with no message.
  - It now has more room to run on Windows.
  - If it still closes, it leaves a crash file saying why, and the diagnostics report includes it. Please send us that report.
- **Updates are signed.** Every update is now signed by NS2Bench's release key, and the app installs nothing whose signature doesn't match. It also refuses an older version offered under a newer number.
- **No more stuck buttons:**
  - If the page goes quiet while you hold a button (a laptop going to sleep, Wi-Fi dropping), NS2Bench lets go within a few seconds.
  - "Release everything" also lets go of mouse buttons.
  - A test or sequence that stops partway releases what it was holding.
  - Two fingers on the same on-screen button no longer release it early.
- **Live view:**
  - Smoother and lighter: no extra work after reconnecting, and no decoding while the tab is hidden.
  - The first frame after connecting is no longer dropped.
  - Reconnecting slows down while the engine is away instead of retrying every second.
- **Sound** comes back after a dropped connection on macOS and Safari instead of staying silent.
- **Faster and steadier capture:**
  - Frames are handled with less copying and less scanning.
  - Changing video settings waits for the card to be released, so Windows no longer reports it as busy.
  - Screen-change checks are about 40% faster on 4K.
- **Reading the screen (OCR)** gives up after 30 s instead of waiting forever, and finds Tesseract on Windows even when it isn't on PATH.
- **Board (NS2Bench Bridge):**
  - Pro Controller mode stays on while the console is asleep. It no longer switches back to the standard controller, or saves that switch, without asking.
  - Restarting the Bridge no longer leaves two copies running.
  - Slow replies no longer hold up the controller.
  - The recovery script keeps going when you run it over ADB.
- **Serial and CH9329:**
  - Windows sets fast serial speeds such as 921600 properly.
  - The CH9329 pointer no longer lags behind fast mouse moves.
  - A stuck CH9329 adapter no longer freezes input.
- **Privacy:** text typed by an agent is no longer written to logs or test reports, only its length.
- **Other fixes:**
  - Adding NS2Bench to an agent no longer removes your existing setup if the add fails.
  - A failed clip no longer leaves a broken video in the list.
  - The offline network helper works on Linux with IPv6 turned off.
  - On macOS, an interrupted update no longer blocks the next one.
  - The diagnostics report no longer stalls on long program output.

## 0.0.14

- **Updates now find, download and install themselves.**
  - NS2Bench checks at start, every 6 hours, and whenever you open Settings → Updates (before, it checked once a day and the Updates page showed an old answer).
  - New versions download in the background and install when you quit NS2Bench, or right away with **Restart now**.
  - Each part can be turned off in Settings → Updates.
- **Close to the menu bar / system tray.** Closing the window keeps NS2Bench running in the menu bar (macOS) or system tray (Windows, Linux), so agents, tests and the engine keep working. The icon brings the window back and has Quit. Minimizing to the tray is a separate option (Windows, Linux). Settings → Appearance → Window. Opening NS2Bench again brings back the running window instead of starting a second copy.
- macOS install help: the README, FAQ, setup guide, website download cards and each release page explain the "Apple could not verify… is free of malware" message and how to allow NS2Bench once.
- Fixes from an audit:
  - "Restart now" no longer runs the installer twice, and checking again no longer throws away a finished download.
  - Windows asks the capture card for its exact frame rate (59.94), opens serial ports COM10 and up, and finds cards that also carry sound.
  - A card picked automatically is kept, so its sound pairs with it, and virtual cameras (OBS, webcam apps) are skipped.
  - Wake presses the power button only after 6 seconds of no signal, never during a dark loading screen.
  - Reports never include a `--token` value.
  - Opening NS2Bench a second time no longer reports that the last run "ended unexpectedly".

## 0.0.13

- **Reports that help fix problems:**
  - **Help → Save bug report** now holds everything needed in one zip:
    - the system (OS version, WebView2 on Windows, ffmpeg and the other parts NS2Bench uses);
    - the capture cards, sound devices and serial ports NS2Bench sees;
    - the check;
    - the last two days of logs, startup traces and crash reports;
    - settings with secrets removed.
  - **When the window won't open:** `ns2bench report` or `ns2bench-app report` saves the same report to Downloads.
  - **Crash files:** if NS2Bench stops unexpectedly, it saves the reason in `~/.ns2bench/logs/crash-….txt`. On Windows it also says where the file is.
  - **After an unexpected close:** the next start offers to save a report.
  - **Safe mode:** `ns2bench-app --safe` starts without capture, input or sound, for when a device makes NS2Bench crash.
  - **Help → Open logs folder** opens the logs folder.
- **Windows:**
  - when no capture card is chosen, NS2Bench uses the first one it finds instead of failing;
  - cards that don't report their sizes still appear in the Video panel;
  - ffmpeg no longer opens console windows.

## 0.0.12

- The Discord link in the app (Help and About), README and website now says **UndefinedLab Discord**, the PhantomPtr server, and uses a permanent invite.

## 0.0.11

- **Windows: capture cards show up by name.** The Video panel lists every DirectShow capture card (internal PCIe cards such as the **Elgato 4K Pro** included) with the sizes and frame rates it offers. Before, Windows showed only a raw list you couldn't pick from. Linux lists its /dev/video cards the same way.
- **Windows: cards that don't offer exactly 1080p60 start anyway.** NS2Bench asks for a size and frame rate the card offers, and if the card still refuses, uses the card's own default (as OBS does).
- **Windows: the card's sound is found by name** ("Game Capture 4K Pro" and similar), and the Video panel has a **Sound** picker on Windows and Linux.

## 0.0.10

- **Network helper (macOS):** it now takes requests only from the console's adapter. Before, it also heard other adapters' broadcasts, for example your home router's address requests, and answered them on the console network. Its answers never left the console adapter, but it shouldn't listen anywhere else. After updating, click **Settings → Install → Network helper → Update helper**.

## 0.0.9

- **Update progress:** Settings → Updates shows a progress bar (MB and percent) while an update downloads, and offers **Install and restart** when it's ready.
- **Windows:** a serial input device (Pico, CH9329) that is plugged in but never answers no longer ties up a thread for good, as on macOS and Linux.
- Waking is safer:
  - the console counts as awake only when the picture shows something (not a black or "No Signal" screen);
  - the power button is never pressed when there's no picture to judge by, or while the console's USB shows it is on;
  - one power action runs at a time.
- The saved power-button command (which may hold a webhook address) is shown only in the app window, kept private on disk, and kept out of the logs.
- HDMI-CEC finds libcec where Homebrew and the Pulse-Eight installer put it, and works with every console address.
- Fixed: the Reconnect button restarted the sound in a loop when sound was on.
- Fixed: messages in the corner showed behind the Settings window.
- Fixed: shortcuts fired while a dialog was open, and repeated while a key was held.
- Preferences: choices made in two windows no longer undo each other, a watch-only browser tab keeps its own choices, and What's new no longer comes back after it was shown.
- One-click agent setup says why it can't add NS2Bench when the engine asks for an API token, instead of adding an entry that can't connect.

## 0.0.8

- The README, FAQ and website now say it plainly: no jailbreak needed, and a jailbroken Switch 2 works too.

- **Wake the console from Sleep Mode or a crash.** Settings → Devices → **Power and wake**: HDMI-CEC through a USB-CEC adapter (nothing mechanical; the console's Match TV Power State must be on), and a power-button command for a SwitchBot-style presser, a Home Assistant webhook or a relay. **Wake the console** is on the no-picture screen, in the Ctrl+K search, in the API (`POST /api/power`), as the `power` MCP tool and as `ns2bench power wake|sleep|button`. Only you can set the command; agents can only ask for a wake or a press. Run the check says whether a way to wake is set up.

## 0.0.7

- Easier to find your way:
  - a red bar says at once when the app loses its engine, with **Reconnect now**
  - with no picture, the screen itself says why and what to check, with buttons to the Video settings and the check
  - results show as short messages in the corner (screenshot saved, clip saved, errors)
  - NS2Bench asks before anything that interrupts the console or removes something (installing on the board, flashing a Pico, installing an update, removing the helper, the command or an agent)
  - **Ctrl+K / ⌘K** searches every action and setting; **?** lists the keyboard shortcuts (F fullscreen, S screenshot, M sound, "," settings)
  - **What's new** appears once after an update, and Settings → Updates shows release notes formatted
- Settings → Devices: a **Calibrate** button (Help pointed at one that didn't exist); after Apply it says right there whether the Bridge connected and the console accepted it; a pasted `address:port` is split into the two fields; serial ports are suggested.
- Help: **Run the check** has a button next to each problem that goes where it is fixed. **Show getting started** brings the Get started card back; each of its steps is a link, with what is still wrong under it, and a fourth step to connect an agent.
- Appearance: **Reset appearance and layout**. About: copy buttons on the support addresses.
- Fixed: the page saved its preferences again every second, and a browser tab without the app's key kept retrying and filled the log with refused requests.

- **Connect an agent in one click.** Settings → API & MCP shows the MCP address and a card each for Claude Code, Codex and OpenCode: whether it has NS2Bench, an Add or Remove button, and the command to run yourself. Agents now connect to the running app by address (`http://127.0.0.1:7630/mcp`), with nothing to install or point at.
- Settings → API & MCP is split into Connect an agent, Agent tools and API, with copy buttons on every command.

## 0.0.6

- `ns2bench serve` now prints a link ending in `?ui=…`. Open that link to drive from your browser. A page opened without it can watch but not drive, so other programs on the computer can no longer pose as you. The desktop app is unchanged.
- Fixed: a serial input device (Pico, CH9329) that was plugged in but never answered slowly used up the engine's threads on macOS and Linux. The port is also set up more reliably on macOS.
- Keyboard & mouse mode: Ctrl+Alt+U gives the keyboard back to the app, so you can leave the picture without a mouse.
- Joy-Con tooltips appear in the right place at every Text size.
- More of the interface is translated: Joy-Con button names, input warnings, update status and a few short messages.

## 0.0.5

- New round buttons in the top bar: reconnect the picture, sound and status; open Updates (a dot shows when one is ready); a bell that lists recent notifications with an unread count; and the light/dark switch.
- Your choices are remembered: theme, text size, language, layout and notification settings are saved in `~/.ns2bench/ui.json`, so they are the same in the app and in a browser and survive a cleared browser.
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
- Settings has a sidebar, and a new Appearance section. It has theme cards, a Text size setting (70–140%) and Accessibility options: Motion, Reduce transparency, High contrast and a dyslexia-friendly font.
- Logs have levels: Everything, Info, Problems and Errors. Each level has an icon, and known problems now explain what happened and what to do. Logs are also saved to `~/.ns2bench/logs` (7 days). More things are logged: API calls, refused requests, the picture stopping or running slow, sound problems, and settings changes.
- Desktop notifications: when the Bridge or the picture drops, when tests and clips finish, when an agent starts driving, and when an update is ready (Settings → Notifications).
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
