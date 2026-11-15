# JavaScript alternatives to the Pico input device

Research note, 2026-11. Question: can JavaScript replace the Raspberry Pi Pico (or the Linux
board) as the thing that presses buttons on the console?

Short answer: **not by itself.** The console only accepts input from a USB (or Bluetooth)
device it recognizes, so *something* must act as a USB device with a controller identity the
console accepts (today: HORI 0f0d:0092 or the Switch 2 Pro Controller 057e:2069). JavaScript can
drive such a device, and some JavaScript runtimes can *be* a USB HID device, but none we found
can present a console-accepted controller identity.

## 1. Browser JavaScript

| API | Can it act as the controller? | Useful for |
|---|---|---|
| WebHID / WebUSB | No. Both are host-side: the browser talks *to* devices, it never becomes one. | — |
| Web Bluetooth | No. Central role only; it cannot advertise as a peripheral/controller. | — |
| Web Serial (Chromium) | No, but it can **drive** a serial input device directly. | A page could talk to a Pico (NS2Bench Bridge serial protocol) or a CH9329 without the NS2Bench engine. |
| Gamepad API | Reads local gamepads only. | Already used: a local gamepad is passed through to the console. |

So the browser is a *client* option, not a device option. NS2Bench already covers that: the
web UI sends input to the engine, which owns the device. A Web Serial path would only remove
the engine from the loop and lose its safety features (press stretching, release on
disconnect, agent locks), so it is not worth building.

## 2. JavaScript on a microcontroller

| Runtime | USB HID device? | Console-acceptable identity? |
|---|---|---|
| Espruino (its own STM32 "Espruino Pico" board, not the Raspberry Pi Pico) | Yes: `E.setUSBHID` sets a custom report descriptor (keyboard/mouse/joystick). | Not documented: no way to set the USB vendor/product ID was found, and the console checks it. |
| Kaluma (RP2040/RP2350) | No USB HID device support found. | — |
| Moddable SDK (ESP32-S3, Pico) | No USB HID API documented; ESP-IDF/TinyUSB has HID underneath but Moddable does not expose it. | — |

Even where HID works, timing is a concern: JavaScript runtimes add garbage-collection pauses,
and the console needs presses held at least 50 ms with predictable releases. The Rust Pico
firmware (`firmware/pico`) already gives a fixed identity, ~1 ms USB polling and the same
protocol as the Linux board, so a JS runtime would be a step backwards unless it brought a
cheaper or more available board.

## 3. Code running on the console

A homebrew program on the console could, in principle, inject input through the system's
HID services. That needs privileges a normal homebrew process does not have (a debug HID
service open was refused in our loader experiments), it is tied to console firmware versions,
and it would run inside the system under test, which defeats the point of an external bench.
Not pursued.

## Decision

Keep hardware input (Linux board, Pico, CH9329). JavaScript stays on the client side: the web
UI, the HTTP API and MCP. Revisit only if a JS microcontroller runtime gains settable USB
vendor/product IDs and deterministic timing.

Sources: [Espruino USB HID](https://www.espruino.com/USB),
[Kaluma](https://github.com/kaluma-project/kaluma),
[Moddable ESP32 docs](https://moddable.com/documentation/devices/esp32),
[ESP-IDF USB device (TinyUSB HID)](https://docs.espressif.com/projects/esp-idf/en/v5.0.2/esp32s3/api-reference/peripherals/usb_device.html).
