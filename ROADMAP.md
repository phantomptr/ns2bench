# Roadmap

Ideas for making NS2Bench more useful, roughly by value. Suggestions and hardware reports
are welcome on [GitHub](https://github.com/phantomptr/ns2bench/issues/new/choose) or
[Discord](https://discord.gg/fzK3xddtrM).

## Next

- **Wake from sleep.** Keep the Bridge powered when the console sleeps (separately powered
  board, no back-feeding) and test waking with a HOME press; or HDMI-CEC "Match TV Power
  State" through a CEC-capable device. Today the console must be woken with its button.
- **Test recorder.** Drive by hand while NS2Bench writes the steps (presses, waits, screen
  checks) into a test file you can replay and edit.
- **Visual checks.** "Expect this region to look like this screenshot" with a tolerance,
  next to the existing text checks, for screens without text.
- **Test suites and schedules.** Run a folder of tests, nightly or on demand, with an HTML
  report (screenshots, clips, timings) and a JUnit file for CI systems.
- **More console profiles.** Screens and routes for System Settings sub-pages, eShop-free
  areas and common homebrew menus, shared as JSON profiles.

## Agents

- **Richer MCP tools.** "Find and select this text on screen", "scroll until", "compare with
  the last screenshot", and a tool that returns the screen as text plus element positions.
- **Agent sessions.** Record an agent's run (prompts, actions, screenshots) as a replayable
  test and a shareable report.
- **Safety rails.** Per-agent allow-lists of buttons and routes, rate limits, and a time box
  after which agent input stops automatically.

## Hardware

- **More Bridges.** Verified guides for Raspberry Pi Zero 2 W and Pi 4/5, an ESP32-S3 USB
  firmware, and a Pico W over Wi-Fi so the Pico needs no UART adapter.
- **Pro mode on more devices.** The Switch 2 Pro Controller identity on the Pico.
- **Several consoles.** One NS2Bench window per console, or a grid, for test farms.

## Everyday use

- **Streaming.** Low-latency WebRTC view for phones and other rooms on the same network.
- **Macros.** Named button sequences on a hotkey (and for agents).
- **Accessibility.** Remappable controls, sticky buttons and one-switch scanning so anyone
  can play with the input they have.

## Project

- Signed and notarized builds (no first-launch warnings).
- Hardware-in-the-loop CI on a real console for each release.
