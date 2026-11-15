# 设置 NS2Bench

NS2Bench 让你（或 AI 代理）可以在电脑上看到、听到并操作 Nintendo Switch 2。
本指南介绍如何连接各个部件。

![NS2Bench 设置一览](overview.svg)

每种设置都由两个相互独立的部分组成：

- **眼睛（必需）：** 主机的 HDMI 接入 USB 采集卡，采集卡向电脑提供视频和音频。
- **双手（可选）：** 一个小型输入设备插入主机的 USB 端口，电脑向它发送指令。主机看到的是有线手柄、键盘和鼠标。

人可以在浏览器或桌面应用中使用 NS2Bench 页面。代理则在同一台电脑上使用 HTTP API 或 MCP（`127.0.0.1:7630`）。所有人共用一个测试台，你能实时看到代理的每一个操作。

**主机始终保持离线。** NS2Bench 绝不会让它连上互联网。采集和 USB 输入完全不需要网络。

## 我需要哪种设置？

| 你手上有什么 | 设置 | 设置中的输入连接 |
|---|---|---|
| 只有采集卡 | [仅查看](#view-only) | 仅查看 |
| 支持 USB 设备模式的 Linux 板（Luckfox Pico Pro/Max、Raspberry Pi Zero 2 W、Pi 4/5） | [Linux 板](#linux-usb-gadget-board) | 网络 Bridge |
| Raspberry Pi Pico（RP2040）和 3.3 V USB 转 UART 适配器 | [Pico](#raspberry-pi-pico) | 串口 Bridge |
| CH9329 模块（仅键盘和鼠标） | [CH9329](#ch9329-module) | CH9329 |
| 你还想让主机接入本地网络（自制软件工具） | [离线网络](#optional-offline-console-network) | （网络标签页） |

不确定？先从**仅查看**开始，之后再添加输入设备。修改输入设置无需重启应用。

如果你需要手柄的全部按键（包括 C、GL 和 GR），请使用 Linux 板。它的 **Pro** 模式会让主机看到一个 Switch 2 Pro手柄。Pico 和 Linux 板的标准模式支持除 C、GL、GR 以外的所有按键。CH9329 完全没有手柄功能。

## 开始之前：你的电脑

从[最新版本](https://github.com/phantomptr/ns2bench/releases/latest)安装应用，然后：

### macOS（14 或更新版本）

- **无需额外安装。** 采集、声音和屏幕文字识别都已内置在应用中。
- 首次启动：如果 macOS 提示应用已损坏或无法验证，运行一次 `xattr -dr com.apple.quarantine /Applications/NS2Bench.app`。
- 系统询问时，请允许**相机**（画面）、**麦克风**（主机声音）和**本地网络**（输入设备）。之后可在“系统设置 → 隐私与安全性”中修改。
- **片段**（保存片段）需要 ffmpeg：`brew install ffmpeg`。
- 串口适配器（Pico、CH9329）无需驱动，会显示为 `/dev/cu.usbserial-…` 或 `/dev/cu.usbmodem…`。

### Windows 10/11

- **安装 ffmpeg**（采集、声音和片段都要用到）：`winget install Gyan.FFmpeg`，然后重启 NS2Bench。NS2Bench 也能找到通过 Chocolatey 或 Scoop 安装的 ffmpeg，或放在 `ns2bench-app.exe` 旁边的 `ffmpeg.exe`。
- SmartScreen 可能提示“Windows 已保护你的电脑”：请选择 **更多信息 → 仍要运行**。
- USB 转 UART 适配器可能需要安装厂商驱动（CH340/CH341 或 CP210x）。端口会在设备管理器中显示为 `COM3` 之类的名称。
- 可选：安装 [Tesseract](https://github.com/UB-Mannheim/tesseract/wiki) 以识别屏幕文字。
- 离线主机网络服务无法在 Windows 上运行：请使用 [Linux 辅助机或路由器](#optional-offline-console-network)。

### Linux

- **ffmpeg：** `.deb` 安装包会自动安装它。使用 AppImage 时需要自行安装（`sudo apt install ffmpeg`、`sudo dnf install ffmpeg`、`sudo pacman -S ffmpeg`）。
- **权限：** 把自己加入拥有采集卡和串口的用户组，然后注销并重新登录：
  `sudo usermod -aG video,dialout $USER`（Arch：`video,uucp`）。
- AppImage：`chmod +x NS2Bench-*.AppImage`。如果无法启动，请安装 FUSE 2（`sudo apt install libfuse2`）。
- 可选：`sudo apt install tesseract-ocr` 用于屏幕文字识别。
- 串口显示为 `/dev/ttyACM0`（Pico）或 `/dev/ttyUSB0`（大多数适配器）。

## 采集卡（所有设置都需要）

- 任何 USB **UVC** HDMI 采集卡都可以，目标是 1080p60。已测试：UGREEN 15389（MS2130）。
- 把主机底座的 **HDMI 输出**接到采集卡的 HDMI 输入，再用 USB 把采集卡接到电脑。
- 在**视频**面板中选择采集卡、分辨率、帧率和画质。列表只显示你的采集卡支持的选项。
- **音频**也来自采集卡，不需要单独的音频线。
- **macOS：** 由一个内置的小助手负责采集。它可以和 OBS 共用采集卡：如果 OBS 已经占用了采集卡，NS2Bench 会沿用 OBS 的格式，绝不会抢走采集卡。首次启动时，请允许**相机**（画面）、**麦克风**（主机音频）和**本地网络**（Bridge）。
- **Linux 和 Windows：** NS2Bench 使用 `ffmpeg`（请先安装）。采集卡可能需要处于空闲状态，如果画面没有出现，请关闭 OBS 或其他占用它的应用。
- 运行 `ns2bench devices` 可列出 NS2Bench 能识别的采集卡。

## 仅查看

![仅查看：一张采集卡，没有输入设备](view-only.svg)

最简单的测试台。你可以实时观看、录制、截取全分辨率截图并识别屏幕文字（OCR）。代理也可以观看，但谁都不能按键。

**所需部件**

- 放在底座上的主机
- USB HDMI 采集卡（UVC）
- HDMI 线和采集卡的 USB 线

**步骤**

1. 把底座的 HDMI 输出接到采集卡。
2. 把采集卡插到电脑上。
3. 启动 NS2Bench 并打开 `http://127.0.0.1:7630`（或使用桌面应用）。
4. 在**视频**面板中选择你的采集卡。
5. 在 **设置 → 设备** 中，把**输入连接**设为**仅查看**，然后点击**应用输入连接**。

**确认是否正常工作**

- 顶部栏的 **video** 状态标签变绿，主机画面出现。
- **Bridge** 状态标签显示 *not configured*。这里属于正常情况。

**常见问题**

- *画面全黑：* 唤醒主机；确认主机已放入底座且底座已通电。换一根 HDMI 线试试。
- *列表中没有采集卡：* 在 macOS 上，请允许相机访问。在 Linux 或 Windows 上，请关闭正在使用采集卡的应用。
- *没有声音：* 在 macOS 上，请允许麦克风访问。检查采集卡是否被选为音频来源。

## Linux USB-gadget 板

![Linux 板：USB 接主机，Ethernet 接电脑](input-linux-board.svg)

一块小型 Linux 板模拟成 USB 设备。它运行 **NS2Bench Bridge**（`ns2bench-board`）。这是经过测试的方案（Luckfox Pico Pro/Max），支持两种手柄模式：

- **标准**（默认）：主机看到的是 HORI 风格的有线手柄 + USB 键盘 + USB 鼠标。
- **Pro**：主机看到的是 Switch 2 Pro手柄，包含 C、GL、GR 在内的所有按键。此模式下没有键盘和鼠标。

在“操作中”面板里用**主机识别为**切换模式，大约需要 8 秒。

**所需部件**

- 支持 USB 设备（OTG）模式的 Linux 板：Luckfox Pico Pro/Max、Raspberry Pi Zero 2 W，或使用 USB-C 端口的 Pi 4/5
- 一根 USB 线，从板子的 **OTG** 端口连到底座的 USB 端口
- 一根连到电脑的 Ethernet 线（或使用 Wi-Fi 或板子的 USB 网络）
- 上面介绍的采集卡设置

**步骤**

1. 在应用中把 NS2Bench Bridge 安装到板子上：**设置 → 安装 → Bridge 开发板**，填入它的 SSH 主机，点击**安装到开发板**。（从源码：`make board-install BOARD=<ssh-host>`；如果是 64 位 Raspberry Pi，请加上 `BOARD_TARGET=aarch64-unknown-linux-musl`。）
2. 把板子的 **OTG** USB 端口接到底座的 USB 端口。
3. 把板子的 Ethernet 接到电脑（或让两者加入同一个私有网络）。
4. 如果 Bridge 尚未运行，请启动它：`ssh <ssh-host> /opt/ns2bench/ns2bench-hidd.sh start --bind <board-ip>`。
5. 确保电脑上的令牌文件 `~/.ns2bench/hidd.token` 与板子上的令牌一致。
6. 在 **设置 → 设备** 中设置：
   - **输入连接：** 网络 Bridge
   - **Bridge 地址：** 板子的 IP（默认 `192.168.79.2`）
   - **Bridge 端口：** `7631`
   - **Bridge 令牌文件：** `~/.ns2bench/hidd.token`
7. 点击**应用输入连接**。

**确认是否正常工作**

- **Bridge** 状态标签变绿，并显示往返时间和 *usb configured*。
- 按一下屏幕上 Joy-Con 的按键。主机会有反应，按键也会亮起。

**主机休眠时**

由主机 USB 供电的板子会在主机休眠时断电，主机唤醒后再重新启动。让 NS2Bench 保持运行：它会在约三秒内察觉并自动重连。等板子启动完毕、主机完成 USB 设置后，输入即可恢复。

要做到这一点，板子必须在开机时自动恢复。为你的板子添加一个开机脚本，先运行 `/opt/ns2bench/gadget.sh`，再运行 `/opt/ns2bench/ns2bench-hidd.sh start`。请用你的板子和底座实际测试一次休眠和唤醒。

**常见问题**

- *Bridge offline · reconnecting：* 检查线缆和板子的 IP。ping 一下板子，确认 Bridge 正在板子上运行。
- *Bridge 已连接但 usb 未配置：* 唤醒主机。检查线缆是否接在板子的 **OTG** 端口，而不是仅供电的端口。有些线缆只能供电。
- *在 HOME 菜单上鼠标没反应：* 正常。主机会读取鼠标，但只有部分软件会显示光标。
- *键盘没反应：* 主机的 USB 键盘支持在“设置 → 手柄与配件”中。
- 在板子上，只能用 `ns2bench-hidd.sh` 和 `gadget.sh` 来停止和启动。不要按名称模式结束进程。

## Raspberry Pi Pico

![Raspberry Pi Pico：USB 接主机，UART 接电脑上的 USB 适配器](input-pico.svg)

运行 NS2Bench Bridge 固件的 Raspberry Pi Pico（RP2040）。它的 USB 端口接主机，呈现的手柄 + 键盘 + 鼠标与标准模式下的 Linux 板相同。电脑通过一个单独的 USB 转 UART 适配器，以串口（UART）链路与它通信。

> Pico 固件已经构建完成，但还没有在真实硬件上运行过。欢迎反馈。

**所需部件**

- Raspberry Pi Pico（RP2040）和一根 micro-USB 线
- 一个 **3.3 V** USB 转 UART 适配器（CP2102、FT232 或 CH340）
- 三根杜邦线
- 上面介绍的采集卡设置

**步骤**

1. 在应用中刷写 Pico：**设置 → 安装 → 刷写 Pico**，然后按住 **BOOTSEL** 按钮，同时把 Pico 插到**电脑**上。应用会把固件复制到 **RPI-RP2** 磁盘，Pico 随即重启。然后拔下它。（从源码：`make pico` 会构建 `firmware/pico/ns2bench-pico.uf2`；请自己把它复制到 RPI-RP2 磁盘。）
2. 把 Pico 和适配器连线：
   - Pico **GP0**（引脚 1，UART0 TX）→ 适配器 **RX**
   - Pico **GP1**（引脚 2，UART0 RX）← 适配器 **TX**
   - Pico **GND**（引脚 3）— 适配器 **GND**
   - 适配器的 3.3 V / 5 V 引脚不要连接。Pico 由主机供电。
3. 把 Pico 的 micro-USB 插到底座的 USB 端口。
4. 把适配器插到电脑上。
5. 在 **设置 → 设备** 中设置：
   - **输入连接：** 串口 Bridge
   - **串口：** 该适配器（例如 macOS 上的 `/dev/cu.usbserial-XXXX`、Linux 上的 `/dev/ttyUSB0`、Windows 上的 `COM3`）
   - **波特率：** `921600`
6. 点击**应用输入连接**。

可选：用 `NS2BENCH_TOKEN=... make pico` 构建，可让 Pico 要求令牌（在电脑上设置相同的令牌文件）。如果你的适配器不支持 921600，请用 `NS2BENCH_BAUD=115200` 构建。

**确认是否正常工作**

- **Bridge** 状态标签变绿，并显示 *usb configured*。
- 在屏幕上的 Joy-Con 按键，主机能收到。

**常见问题**

- *没有出现串口：* 安装适配器的驱动（CP210x、FTDI 或 CH340），然后重新插拔。
- *Bridge offline：* 很可能是 TX 和 RX 接反了。TX 总是接 RX。也请检查 GND。
- *回复乱码或没有回复：* 波特率必须与固件一致（除非你用其他值构建，否则是 921600）。
- *切勿使用 5 V 适配器。* Pico 的引脚只能承受 3.3 V。

## CH9329 模块

![CH9329：USB 接主机，UART 接电脑](input-ch9329.svg)

CH9329 模块把串口指令转换成 USB 键盘和鼠标。它便宜又简单，但**没有手柄功能**：没有按键，也没有摇杆。只能用来打字和指点。

**所需部件**

- 一个 CH9329 模块
- 一个 3.3 V USB 转 UART 适配器（如果模块的线缆内置 USB 串口则不需要）
- 上面介绍的采集卡设置

**步骤**

1. 把模块的 USB（HID）端插到底座的 USB 端口。
2. 把 UART 端与适配器连线：模块 TX → 适配器 RX，模块 RX ← 适配器 TX，GND — GND。（如果模块内置 USB 串口，请跳过此步。）
3. 把适配器（或模块的串口线）插到电脑上。
4. 在 **设置 → 设备** 中设置：
   - **输入连接：** CH9329
   - **串口：** 适配器的端口
   - **波特率：** 模块的设置值，通常为 `9600`
5. 点击**应用输入连接**。

**确认是否正常工作**

- **Bridge** 状态标签变绿。
- 在原生键盘模式下打字，内容会出现在主机上（例如文本框中）。

**常见问题**

- *手柄按键没反应：* 正常。CH9329 只支持键盘和鼠标。
- *什么都没发生：* 检查波特率，必要时交换 TX/RX。
- *主机不认键盘：* 检查“设置 → 手柄与配件 → USB 键盘”。

## 可选：离线主机网络

![让主机接入离线网络的三种方式](network.svg)

只有使用本地网络工具（例如自制软件加载器）时才需要这一部分。采集和输入永远不需要它。

无论选择哪种方式，主机都**无法上网**。NS2Bench 绝不转发流量，绝不开启互联网共享、NAT 或代理，也绝不查询外部 DNS 服务器。它的本地 DNS 只回应主机的连接测试，其他所有域名都会被拒绝。主机尝试的每一次查询都会显示在 **network** 日志中。

电脑到 Linux 板的 Bridge 链路是独立的，可以使用你平常的局域网。

### A · 直连网线（macOS 或 Linux）

**所需部件：** 电脑上一个专门给主机用的空闲 Ethernet 适配器、一根 Ethernet 线，以及底座的 LAN 端口（或主机上的 USB-Ethernet 适配器）。

1. 给这个专用适配器设置固定的私有地址，例如 `192.168.80.1/24`。NS2Bench 不会替你配置网卡。
2. 在 `bench.json` 中描述这个网卡（使用你的适配器的网卡名称）：
   ```json
   "network": { "interface": "en7", "address": "192.168.80.1", "subnet": "192.168.80.0/24",
                "pool_start": "192.168.80.10", "pool_end": "192.168.80.13" }
   ```
   构建网络服务：`make netd`。
3. 在不打开任何端口的情况下检查配置：`native/target/release/ns2bench-netd --check bench.json`。
4. 把它安装为服务：
   - **macOS：** 把二进制文件复制到 `~/.ns2bench/bin/ns2bench-netd`，填好 `launchd/ns2bench-netd.plist.template` 并复制到 `~/Library/LaunchAgents/`。无需 root 权限。
   - **Linux：** 安装到 `/opt/ns2bench/ns2bench-netd`，并使用 systemd 单元 `launchd/ns2bench-netd.service`。
5. 把主机的 Ethernet 接到专用适配器。
6. 在 **设置 → 网络** 中，选择**直连、隔离的电脑 ↔ 主机**。按需开启本地 DHCP、DNS 和连接测试页面，然后应用。

主机会从地址池中获得地址（例如 `192.168.80.10`–`192.168.80.13`）。**网络**标签页会显示实时状态和租约。

如果 IP 转发已开启，或无法检查，服务会拒绝启动。这是有意设计的。

### B · 隔离的路由器、VLAN 或 Wi-Fi（任意系统）

1. 为主机准备一个路由器、VLAN 或 Wi-Fi 网络，并**屏蔽互联网，包括 IPv6**。普通家庭局域网并不是隔离的。
2. 让路由器分配地址（DHCP）。
3. 通过 Ethernet 或 Wi-Fi 把主机连上去。
4. 在 **设置 → 网络** 中，选择**隔离的路由器 / VLAN / Wi-Fi**。这会关闭 NS2Bench 的本地 DHCP 和 DNS，避免与路由器冲突。

### C · Windows 电脑配合远程辅助机

Windows 无法在本地运行网络服务。请改在 Linux 板或 PC 上运行它，再从 Windows 控制。

1. 在 Linux 辅助机上，按照方式 A 设置 `ns2bench-netd`。辅助机负责主机的 Ethernet。
2. 生成令牌：`openssl rand -hex 16 > ~/.ns2bench/netd-control.token`。把同一个文件复制到 Windows 电脑。
3. 在辅助机的配置中，在一个**不在**主机网络上的私有地址上添加管理监听：
   ```json
   "network": { "control": { "listen": "192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
4. 在 Windows 上 NS2Bench 的配置中，指向辅助机：
   ```json
   "network": { "helper": { "url": "http://192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
5. 让管理链路保持私有：使用直连网线或板子的 USB 网络。它的流量是明文 HTTP。

之后，Windows 上的**网络**标签页会显示辅助机的状态和控制项。也可以不用辅助机，改用方式 B。

**常见问题**

- *网络标签页显示不可用：* 服务没有运行，或者服务和界面使用了不同的状态文件夹。在 Windows 上，请检查 `network.helper`，并确认两个令牌文件一致。
- *主机获取不到地址：* 检查配置中的网卡名称和子网，并确认网线接在专用适配器上。
- *主机提示没有互联网：* 这是正确的。连接测试会在本地通过，但不会再往外走。

## 代理

状态标签都变绿后，代理就可以使用测试台了。打开 **设置 → API 与 MCP**，可以看到 API 地址、请求示例，以及可直接复制给 Claude Code、Codex 或 opencode 的 MCP 配置。代理和人共用同一套控制，“仅手动”开关可随时阻止代理输入。

## Windows 说明

- 采集使用带 DirectShow 的 `ffmpeg`。请关闭其他占用采集卡的应用。
- Linux 板、Pico 和 CH9329 都可以在 Windows 上使用。串口名称为 `COM3`、`COM4` 等。
- Windows 上没有本地网络服务。请使用远程辅助机（方式 C）或隔离的路由器（方式 B）。
