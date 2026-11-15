# NS2Bench reference

Hardware details, the HTTP API, tests, calibration, logs and the offline network.
Start with the [README](../README.md) and the [setup guide](setup/README.md).

## Hardware

### Capture (eyes)

Any USB (UVC) HDMI capture card. Tested: UGREEN 15389 (MS2130) — 1080p60, 1440p30.

| OS | Backend | Notes |
|---|---|---|
| macOS | native helper (`avf`) | never steals the card from OBS; applies your format when it is free; hardware JPEG (1080p60 ≈ 12% of one core) |
| Linux | ffmpeg `v4l2` | MJPEG passthrough, no re-encode; set `h264_encoder` (e.g. `h264_vaapi`, `h264_nvenc`, `libx264`) for the H.264 live view |
| Windows | ffmpeg `dshow` | `device` is the DirectShow name; `h264_encoder` e.g. `h264_mf`, `h264_nvenc`, `h264_qsv` |
| anything else | `ffmpeg_input` | your own ffmpeg input arguments |

Pick device, resolution, fps and quality in the UI's **Video** card (lists only what the card offers), or in `bench.json`.

### Input (hands)

| Hardware | Controller | Keyboard | Mouse | Status |
|---|---|---|---|---|
| **Luckfox Pico Pro/Max** (Linux, USB OTG) | yes (HORI or **Switch 2 Pro**) | yes | yes | **tested on Switch 2** (both modes) |
| Raspberry Pi Zero 2 W / Pi 4 / Pi 5 (USB OTG) | yes | yes | yes | same daemon, gadget and FunctionFS code; untested |
| Other Linux boards with USB device mode (configfs) | yes | yes | yes | same code; untested |
| Raspberry Pi Pico (RP2040) | yes | yes | yes | `firmware/pico` (Rust); host link over UART; standard mode only; built, not yet run on hardware |
| CH9329 serial-to-HID module | no | yes | yes | keyboard + mouse only; `"backend": "ch9329"` |

Any Linux OTG board runs **NS2Bench Bridge** (`ns2bench-board`), one static binary (~650 KB, no Python or libc needed; Rust, `native/crates/board`), with `board/gadget.sh` building the USB device in one of two **controller modes** (switch from the Driving panel's *The console sees*, `POST /api/control {"usb_mode": "pro"}`, ~8 s):

| Mode | The console sees | Buttons | Keyboard + mouse |
|---|---|---|---|
| **standard** (boot default) | HORI-style controller (`0f0d:0092`) + USB keyboard + USB mouse | all except C, GL, GR (C works as a GameChat shortcut) | yes |
| **pro** | a **Switch 2 Pro Controller** (`057e:2069`, wired protocol via FunctionFS, `ns2bench-board pro2`) | **all, incl. C, GL, GR** | no |

Pro mode follows GP2040-CE's Switch 2 Pro driver (MIT): the two-interface layout at full speed, report `0x09`, the bulk command protocol including pairing (AES-128), firmware updates ignored, a stable identity. Verified on hardware: handshake + pairing, player 1 LED, all 18 test-screen buttons incl. GL/GR, C opens GameChat. (A Pro Controller with extra keyboard/mouse interfaces enumerates, but the console never reads the extras, so keyboard/mouse live in standard mode.) The Switch 2 polls the USB mouse in standard mode; whether a cursor appears depends on the software (the HOME menu shows none).

Easiest: in the app, **Settings → Install → Bridge board**: enter the board's SSH host (key sign-in), click *Install on board*. It downloads the board binary for this app version (checked against the release's `SHA256SUMS`), uploads the matching scripts, keeps the current files as `*.bak`, and restarts the daemon. From a source checkout:

```sh
make board-install BOARD=<ssh-host>          # cross-build, upload, syntax-check, keep *.bak, restart
make board-install BOARD=<ssh-host> BOARD_TARGET=aarch64-unknown-linux-musl   # 64-bit Raspberry Pi
ssh <ssh-host> /opt/ns2bench/ns2bench-hidd.sh start --bind <board-ip>
```

The scripts require `/opt/ns2bench/ns2bench-board` and refuse a USB mode change if the binary is missing. Golden fixtures preserve compatibility with the original protocol. Measured on the Luckfox: 0.7 MB resident, host round trip 1.6 ms.

### Raspberry Pi Pico

The Pico's USB port goes to the console and shows exactly what the Linux boards show in standard mode (HORI-style controller + keyboard + mouse, same descriptors). The computer talks to it over UART0 through any USB-serial adapter (CP2102, CH340, FTDI): adapter TX → GP1, adapter RX → GP0, GND → GND. Same protocol and watchdog as the Linux daemon.

Easiest: in the app, **Settings → Install → Flash Pico**, then hold BOOTSEL while you plug the Pico into the computer; the app finds the `RPI-RP2` drive and copies the release firmware. From source:

```sh
make pico                                    # -> firmware/pico/ns2bench-pico.uf2
# hold BOOTSEL while plugging the Pico into the computer, copy the .uf2 onto the RPI-RP2 drive,
# then plug the Pico into the console
```

```json
"input": {"backend": "serial", "serial_port": "/dev/cu.usbserial-XXXX", "baud": 921600}
```

Optional build settings: `NS2BENCH_TOKEN=... make pico` makes the Pico require that token in the hello (set the same `token_file` on the host); `NS2BENCH_BAUD=115200` for slow adapters.

### CH9329

A CH9329 module (serial in, USB keyboard + mouse out) gives the console a keyboard and mouse but no controller. Wire it like the Pico (adapter TX → CH9329 RX) and set:

```json
"input": {"backend": "ch9329", "serial_port": "/dev/cu.usbserial-XXXX", "baud": 9600}
```

**Board housekeeping:** `make board-install` also installs `swap.sh` (turns on an SD-card swap partition at boot through `/etc/init.d/S98ns2bench-swap`, an out-of-memory safety net for low-RAM boards) and `restore-default-gadget.sh` (recovery: gives the board its vendor USB gadget back, e.g. RNDIS + ADB on the Luckfox).

The NS2Bench Bridge network connection uses an IP and port: use any network adapter or USB networking you like.

**NS2Bench Bridge** is the input software running on your device: the `ns2bench-board`
Rust daemon on Linux boards, or NS2Bench Bridge firmware on a Raspberry Pi Pico.
It receives controls from NS2Bench and presents USB input devices to the console.
Existing executable names, paths and configuration values such as `ns2bench-board`
and `hidd` remain unchanged for compatibility.
The UI warns when you attempt input while the Bridge or console USB is unavailable;
those attempts are discarded rather than queued for reconnection.

### Recovering after console sleep

If console USB power disappears, a Linux input board must boot again when power returns.
Its boot setup must restore the saved USB gadget, management address and HID daemon.
`board/install.sh` installs the daemon, `gadget.sh` and an SD-swap boot hook, but not a boot
script that starts the gadget and daemon: add one for your board's init system that runs
`/opt/ns2bench/gadget.sh` and then `/opt/ns2bench/ns2bench-hidd.sh start`. Keep NS2Bench running: it reconnects
automatically, detects three seconds without heartbeat replies, bounds blocked writes to
two seconds, and retries failed connections with a one-to-two-second backoff. Connection
attempts can take up to five seconds. Input is available once the board is reachable and
the console has configured USB; a network connection alone does not prove USB readiness.
Boot and console enumeration time are additional to these host retry intervals.

Continuous independent board power avoids the boot delay, but use a board-approved power
arrangement that prevents USB backfeeding. A physical sleep/wake cycle is required to
validate recovery for each board, dock and power arrangement.

## Driving by hand

- **Gamepad:** plug any controller into the computer; the browser Gamepad API maps it by position (bottom face = B).
- **Your keyboard sends → Controller buttons:** click the screen. WASD left stick, TFGH right stick, arrows D-pad, I J K L = X Y B A, Q/E = L/R, 1/3 = ZL/ZR, Enter/Backspace = +/−, ` = HOME, P = Capture, V/N = stick clicks; in Pro mode C = C, Z/X = GL/GR. Shift+click locks the mouse to aim the right stick. "Show key labels on Joy-Con" prints each key inside its button.
- **Your keyboard sends → Keyboard & mouse:** sends a real USB keyboard (and mouse) to the console. The Switch 2 takes a USB keyboard (System Settings → Controllers & Accessories → USB Keyboard).
- **On-screen Joy-Con 2:** click any button; drag a stick; tap a stick to click it. Your presses light up in the side's colour, agent presses in purple, and a trail under the screen lists the last inputs and who made them.

## Agents: MCP and HTTP API

Start the bench (`make serve`, or `ns2bench serve`), then add the MCP server. It is the same `ns2bench` binary (`native/target/release/ns2bench`, or the one inside the desktop app) and talks to the running bench, so whoever watches the UI sees every agent action and can block agent input:

```sh
# Claude Code
claude mcp add ns2bench -- /path/to/ns2bench mcp
```

```toml
# Codex: ~/.codex/config.toml
[mcp_servers.ns2bench]
command = "/path/to/ns2bench"
args = ["mcp"]
```

```json
// opencode: opencode.json
{ "mcp": { "ns2bench": { "type": "local", "command": ["/path/to/ns2bench", "mcp"] } } }
```


Tools (34):
- **See:** `screenshot` (with `region` to crop/zoom), `read_screen` (OCR with positions), `find_text` (every match with its box), `wait_for_text`, `wait_for_screen` (until the screen changes, or until it settles), `where_am_i`, `wait`
- **Drive:** `press`, `hold`, `release`, `stick`, `key`, `type_text`, `mouse_move` (with `buttons` to drag), `mouse_click`, `scroll`, `sequence`, `navigate`, `list_routes`, `usb_mode`, `probe` (press and measure the reaction)
- **Check and debug:** `health` (ready to drive? what to fix), `status`, `doctor`, `logs` (with `level` and `after`), `events` (who pressed what), `diagnostics`, `video_info`
- **Record and test:** `save_clip`, `list_clips`, `run_test`, `list_reports`
- **Control:** `block_agents` (only the person at the bench can undo it), `screen_log`

Most action tools take `look_after_ms` to return a screenshot taken after the action. A good loop: act, then `wait_for_screen` (`until: "change"`), then `screenshot` or `find_text`.

The command line has the same abilities (`ns2bench --help`; `--json` for machine-readable output). Install it from the app: **Settings → Install → Command line tool**.

HTTP (JSON, `127.0.0.1:7630`; binding elsewhere requires `server.token`):

| | |
|---|---|
| `GET /api/status` | video fps, input link, USB state, calibration |
| `GET /api/health` | ready to drive? `{ok, video, input, agents_allowed, problems: [{area, problem, fix}]}` |
| `GET /api/screenshot?format=png&max_width=1280&region=0,0.8,1,0.2` | latest frame (optionally cropped); headers `X-Frame-Seq`, `X-Frame-Age-Ms` |
| `GET /stream.mjpg` | live MJPEG |
| `POST /api/press` `{"buttons":["A"],"hold_ms":100}` | also `hold`, `release`, `stick`, `key`, `type`, `mouse`, `click` |
| `POST /api/sequence` `{"steps":[{"press":["DOWN"]},{"wait_ms":300},{"press":["A"]}]}` | several steps, one call |
| `POST /api/wait_screen` `{"until":"change"\|"stable","timeout_ms":5000,"stable_ms":500,"region":[x,y,w,h]}` | wait for the picture to react, or to stop moving |
| `POST /api/probe` `{"buttons":["RIGHT"],"hold_ms":100,"timeout_ms":1500}` | press and watch the video: did it register, how fast |
| `POST /api/calibrate` | sweep press lengths, save the minimum |
| `GET /api/logs?source=kernel&level=warn&after=SEQ&limit=200&contains=usb` | bench, api, board, kernel, network, screen, your files; levels and hints (see *Logs*) |
| `GET /api/events?source=mcp&limit=30` | who pressed what (ui, api, mcp) |
| `GET/POST /api/video` | devices/formats; change capture settings |
| `GET /api/ocr?region=x,y,w,h&fast=1` · `GET /api/find_text?text=Settings` · `POST /api/wait_text` | read screen text; where a text is; wait for text to appear or go |
| `GET /api/where` · `GET /api/routes` · `POST /api/navigate` | which screen; known routes; go somewhere and verify |
| `POST /api/test` | run a test (see *Testing the console*) |
| `POST /api/clip` · `GET /api/clips` · `GET /clips/{name}` · `GET /api/reports` | save the last N seconds of video; list and download clips; list test reports |
| `GET /api/doctor` · `POST /api/doctor {"probe":true}` | end-to-end health check (the probe presses RIGHT/LEFT on HOME) |
| `GET /api/diagnostics?lines=40` · `GET /api/bundle` | Markdown summary; zip of logs, status, screenshot, config |
| `GET/POST /api/input` · `GET/POST /api/network` · `GET /api/integrations` | input device, console network, MCP/API setup |
| `POST /api/control` | `{"usb_mode": "standard\|hori\|pro"}`, `{"manual_only": true}`, `{"screen_log": true}` |

While **Block agents** (manual-only) is on, agent requests that press, type, click, run tests, probes or calibration, save a clip, switch the USB mode, or change video, input, network or screen-log settings get `409`. Only the NS2Bench page can turn it off (`403` for anyone else), and only the page can change the Bridge address or token file (`403`). Requests from other web pages are refused (`403`) and logged.

The desktop app proves which requests come from its own window with a key it creates at each launch. `ns2bench serve` on its own has no such window, so there any local program that can reach the port can also act as the page: use `server.token` if other users or untrusted programs share the computer.

Buttons: `A B X Y L R ZL ZR PLUS MINUS HOME CAPTURE LSTICK RSTICK UP DOWN LEFT RIGHT`, plus `C GL GR` in Pro mode.

## Testing the console

A test is a JSON file of steps; the bench runs it and writes a report (pass/fail per step, screenshots, the screen text and a video clip of the moments before any failure, plus every log line from the run) to `~/.ns2bench/reports/`:

```json
{"name": "System Settings smoke",
 "steps": [{"navigate": "system_settings"}, {"expect_text": "Airplane Mode"},
           {"screenshot": "settings"}, {"navigate": "home"}, {"probe": ["RIGHT"]}]}
```

```sh
ns2bench test examples/tests/settings-smoke.json             # also POST /api/test, MCP run_test
```

Steps: `press/hold/release/stick/key/type/mouse/click/wait_ms`, `navigate`, `ensure`, `expect_screen`, `expect_text` (`"regex": true`), `wait_text`, `expect_gone`, `screenshot`, `probe`, `note`. Screens and routes come from a **console profile** (`profiles/switch2.json`, overridable in `~/.ns2bench/profiles/`): routes find HOME icons by their label, start by waking the screen, and verify where they land.

## Recorder and doctor

- The bench always keeps the last ~30 s of video (100 MB cap). **Save clip** (header button, `POST /api/clip`, MCP `save_clip`) writes an MP4; failing test steps get one automatically.
- `ns2bench doctor --probe` checks video, the console picture ("No Signal"), OCR, the Bridge connection, USB state, a real press on HOME, calibration, network isolation and the console's lease, and says what to fix.

## Calibration (measured on Switch 2)

`POST /api/calibrate` presses RIGHT/LEFT on a menu with growing hold times and watches the screen.

| press | 8 ms | 16 ms | 24 ms | 33 ms | 50 ms | 66 ms | 100 ms |
|---|---|---|---|---|---|---|---|
| registered | 0/4 | 0/4 | 1/4 | 3/4 | 4/4 | 4/4 | 4/4 |

So every press is held at least **75 ms** (1.5× the shortest reliable press), including quick taps from the browser. Input-to-screen latency, as the bench sees it (console render + capture card): **126 ms median**. The input path itself (computer → board) is ~3 ms of that. Menus react to the left stick from ~70% tilt.

## Logs

The Logs panel (and `GET /api/logs`, MCP `logs`, `ns2bench logs`) collects: **bench** (every human/agent action, errors, reconnects, the picture stopping or running slow), **api** (each API call at *debug*, refused and failed requests at *warn*), **board** (daemon), **kernel** (board kernel log: USB enumeration, gadget errors), **network** (the offline Switch network's DHCP/DNS), **screen** (new on-screen text, when the screen log is on), and any files listed under `"logs"` in the config.

Each line has a level: `debug` (detail such as API calls and page connections), `info`, `warn` (something to look at that may fix itself) and `error` (needs you). For problems NS2Bench recognises, a line also carries a `hint`: what happened and what to do. Lines from files and the board are levelled by their words. Filter with `level=warn` (warnings and errors).

Every line is also written to `~/.ns2bench/logs/ns2bench-YYYY-MM-DD.log` (kept 7 days; `"log_files": false` in the config turns this off). Lines marked `notify` become desktop notifications in the app (Settings → Notifications).

## Offline console network (optional)

`ns2bench-netd` (`native/crates/netd`) serves DHCP, a DNS that answers nothing but the console's connectivity test, and the local test reply, on one interface with no forwarding. The Switch gets a LAN for homebrew loaders without any internet or Nintendo access; every lookup it tries is logged (the *network* log).

| OS | How |
|---|---|
| macOS | **Settings → Install → Network helper** (a user LaunchAgent, no root), or by hand: `launchd/ns2bench-netd.plist.template` (replace `__REPO__`/`__HOME__`, copy to `~/Library/LaunchAgents/`) |
| Linux | **Settings → Install → Network helper** (asks for an administrator password once; runs as you with only `CAP_NET_RAW` + `CAP_NET_BIND_SERVICE`), or the systemd unit `launchd/ns2bench-netd.service` |
| Windows | run netd on a Linux machine or board that owns the console's Ethernet port, and control it remotely (below) |

Build with `make netd`. Validate without opening service ports with `native/target/release/ns2bench-netd --check bench.json`, then run the same command without `--check` to serve. On macOS, copy the binary to `~/.ns2bench/bin/ns2bench-netd` before loading the LaunchAgent template. On Linux, install it at the unit’s `/opt/ns2bench/ns2bench-netd` path; its default state directory is `/var/lib/ns2bench/.ns2bench` (keep any explicit `state_dir` inside `/var/lib/ns2bench`).

The daemon pins every socket to the configured interface, binds DNS/HTTP to its private address on Linux (macOS requires a wildcard address for unprivileged low ports; interface pinning still restricts traffic), and refuses service if IPv4 or IPv6 forwarding is enabled or cannot be verified. It never changes routes, DNS settings, firewall rules or forwarding. Unknown DNS names receive NXDOMAIN locally. Network configuration must already exist; the daemon does not configure the interface.

### Remote helper (Windows hosts)

A host that cannot run netd (Windows) can still use the Network tab: run netd on a Linux machine or board and give it a management port. On the helper, add to its config:

```json
"network": { "...": "...", "control": { "listen": "192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
```

`listen` must be one specific management address that is not on the console network (netd refuses `0.0.0.0` and console-side addresses). The token file holds a random secret of at least 16 characters (`openssl rand -hex 16`). On the host, point the engine at it:

```json
"network": { "helper": { "url": "http://192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
```

The helper accepts only `GET /status` and `POST /controls` with that bearer token, and only the same DHCP/DNS/connectivity switches as the local service; isolation is not a setting. Traffic is plain HTTP, so keep the management link private (a direct cable or the board's USB network).
