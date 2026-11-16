# 設定 NS2Bench

NS2Bench 讓你（或 AI 代理）從電腦上看到、聽到並操作 Nintendo Switch 2。
本指南說明如何接線與設定。

![NS2Bench 設定總覽](overview.svg)

每種設定都包含兩個彼此獨立的部分：

- **眼睛（一定要有）：** 主機的 HDMI 接到 USB 擷取卡，擷取卡把影像和聲音傳給電腦。
- **雙手（選用）：** 一個小型輸入裝置插在主機的 USB 連接埠上，電腦傳送指令給它，主機則把它當成有線控制器、鍵盤和滑鼠。

使用者透過瀏覽器中的 NS2Bench 頁面或桌面應用程式操作；代理則在同一台電腦上使用 HTTP API 或 MCP（`127.0.0.1:7630`）。大家共用同一套測試台，代理的每個動作你都能即時看到。

**主機始終保持離線。** NS2Bench 絕不會讓主機連上網際網路。擷取與 USB 輸入完全不需要網路。

## 我需要哪種設定？

| 你手上有什麼 | 設定 | 設定中的輸入連線 |
|---|---|---|
| 只有擷取卡 | [僅檢視](#view-only) | 僅檢視 |
| 支援 USB 裝置模式的 Linux 開發板（Luckfox Pico Pro/Max、Raspberry Pi Zero 2 W、Pi 4/5） | [Linux 開發板](#linux-usb-gadget-board) | 網路 Bridge |
| Raspberry Pi Pico（RP2040）和 3.3 V USB 轉 UART 轉接器 | [Pico](#raspberry-pi-pico) | 序列埠 Bridge |
| CH9329 模組（僅鍵盤和滑鼠） | [CH9329](#ch9329-module) | CH9329 |
| 你還想讓主機連上本地網路（自製軟體工具） | [離線網路](#optional-offline-console-network) | （網路分頁） |

不確定？先從**僅檢視**開始，之後再加上輸入裝置。變更輸入設定不需要重新啟動應用程式。

如果你需要所有控制器按鈕（包括 C、GL 和 GR），請使用 Linux 開發板。它的 **Pro** 模式會讓主機看到一支 Switch 2 Pro控制器。Pico 和開發板的標準模式支援 C、GL、GR 以外的所有按鈕。CH9329 完全沒有控制器功能。

## 開始之前：你的電腦

從[最新版本](https://github.com/phantomptr/ns2bench/releases/latest)安裝應用程式，然後：

### macOS（14 或更新版本）

- **不需要另外安裝任何東西。** 擷取、聲音和畫面文字辨識都已內建在應用程式中。
- 第一次啟動：如果 macOS 顯示 Apple 無法驗證 .dmg，或顯示應用程式已損毀或無法檢查，請在開啟 .dmg 之前執行 `xattr -dr com.apple.quarantine ~/Downloads/NS2Bench-*.dmg`（安裝後則執行 `xattr -dr com.apple.quarantine /Applications/NS2Bench.app`），或在 系統設定 → 隱私權與安全性 中按一下 **強制打開**。只有第一次安裝時需要：App 內更新不需要。
- 系統詢問時，請允許**相機**（畫面）、**麥克風**（主機聲音）和**區域網路**（輸入裝置）。之後可以在「系統設定 → 隱私權與安全性」中更改。
- **片段**（儲存片段）需要 ffmpeg：`brew install ffmpeg`。
- 序列轉接器（Pico、CH9329）不需要驅動程式，會顯示為 `/dev/cu.usbserial-…` 或 `/dev/cu.usbmodem…`。

### Windows 10/11

- **安裝 ffmpeg**（擷取、聲音和片段都會用到）：`winget install Gyan.FFmpeg`，然後重新啟動 NS2Bench。NS2Bench 也能找到用 Chocolatey 或 Scoop 安裝的 ffmpeg，或放在 `ns2bench-app.exe` 旁邊的 `ffmpeg.exe`。
- SmartScreen 可能會顯示「Windows 已保護您的電腦」：請選擇**其他資訊 → 仍要執行**。
- USB 轉 UART 轉接器可能需要廠商驅動程式（CH340/CH341 或 CP210x）。連接埠會在裝置管理員中顯示為 `COM3` 之類的名稱。
- 選用：安裝 [Tesseract](https://github.com/UB-Mannheim/tesseract/wiki) 以辨識畫面文字。
- 離線主機網路服務無法在 Windows 上執行：請使用 [Linux 輔助裝置或路由器](#optional-offline-console-network)。

### Linux

- **ffmpeg：** `.deb` 會自動幫你安裝。若使用 AppImage，請自行安裝（`sudo apt install ffmpeg`、`sudo dnf install ffmpeg`、`sudo pacman -S ffmpeg`）。
- **權限：** 把自己加入擁有擷取卡和序列埠的群組，然後登出再登入：
  `sudo usermod -aG video,dialout $USER`（Arch：`video,uucp`）。
- AppImage：`chmod +x NS2Bench-*.AppImage`。如果無法啟動，請安裝 FUSE 2（`sudo apt install libfuse2`）。
- 選用：`sudo apt install tesseract-ocr` 以辨識畫面文字。
- 序列埠會顯示為 `/dev/ttyACM0`（Pico）或 `/dev/ttyUSB0`（大多數轉接器）。

## 擷取卡（所有設定都需要）

- 任何 USB **UVC** HDMI 擷取卡都能用，目標是 1080p60。已測試：UGREEN 15389（MS2130）。
- 把主機底座的 **HDMI 輸出**接到擷取卡的 HDMI 輸入，再用 USB 把擷取卡接到電腦。
- 在**影像**面板中選擇擷取卡、解析度、影格率和畫質。這裡只會列出你的擷取卡支援的選項。
- **聲音**也來自擷取卡，不需要另外的音訊線。
- **macOS：** 由一個內建的小型輔助程式負責擷取。它會和 OBS 共用擷取卡：如果 OBS 已經佔用擷取卡，NS2Bench 會沿用 OBS 的格式，絕不會搶走擷取卡。第一次啟動時，請允許**相機**（畫面）、**麥克風**（主機聲音）和**區域網路**（Bridge）。
- **Linux 和 Windows：** NS2Bench 使用 `ffmpeg`（請先安裝）。擷取卡可能需要處於空閒狀態，若畫面沒有出現，請關閉 OBS 或其他佔用它的應用程式。
- 執行 `ns2bench devices` 可列出 NS2Bench 能看到的擷取卡。

## 僅檢視

![僅檢視：一張擷取卡，沒有輸入裝置](view-only.svg)

最簡單的測試台。你可以即時觀看、錄影、拍攝全解析度截圖，以及辨識畫面文字（OCR）。代理也能觀看，但沒有人能按按鈕。

**所需零件**

- 放在底座上的主機
- USB HDMI 擷取卡（UVC）
- HDMI 線和擷取卡的 USB 線

**步驟**

1. 把底座的 HDMI 輸出接到擷取卡。
2. 把擷取卡插到電腦上。
3. 啟動 NS2Bench 並開啟 `http://127.0.0.1:7630`（或使用桌面應用程式）。
4. 在**影像**面板中選擇你的擷取卡。
5. 在**設定 → 裝置**中，把**輸入連線**設為**僅檢視**，然後按一下**套用輸入連線**。

**確認是否正常運作**

- 頂端列的**影像**狀態標籤變成綠色，並出現主機畫面。
- **Bridge** 狀態標籤顯示*未設定*。這在此設定中是正常的。

**常見問題**

- *畫面全黑：* 喚醒主機；確認主機已放在底座上，且底座有供電。試試另一條 HDMI 線。
- *清單中沒有擷取卡：* 在 macOS 上，請允許相機存取。在 Linux 或 Windows 上，請關閉正在使用擷取卡的應用程式。
- *沒有聲音：* 在 macOS 上，請允許麥克風存取。確認擷取卡是目前選擇的音訊來源。

## Linux USB gadget 開發板

![Linux 開發板：USB 接主機，Ethernet 接電腦](input-linux-board.svg)

一塊小型 Linux 開發板模擬成 USB 裝置，並執行 **NS2Bench Bridge**（`ns2bench-board`）。這是已經過測試的設定（Luckfox Pico Pro/Max）。它支援兩種控制器模式：

- **標準**（預設）：主機看到的是 HORI 風格的有線控制器 + USB 鍵盤 + USB 滑鼠。
- **Pro**：主機看到的是 Switch 2 Pro控制器，包含 C、GL 和 GR 在內的所有按鈕。此模式下沒有鍵盤和滑鼠。

在操作面板中用**主機識別為**切換模式，大約需要 8 秒。

**所需零件**

- 支援 USB 裝置（OTG）模式的 Linux 開發板：Luckfox Pico Pro/Max、Raspberry Pi Zero 2 W，或使用 USB-C 連接埠的 Pi 4/5
- 一條從開發板 **OTG** 連接埠接到底座 USB 連接埠的 USB 線
- 一條接到電腦的 Ethernet 線（或使用 Wi-Fi 或開發板的 USB 網路）
- 上述的擷取卡設定

**步驟**

1. 從應用程式把 NS2Bench Bridge 安裝到開發板上：**設定 → 安裝 → Bridge 開發板**，輸入它的 SSH 主機，按一下**安裝到開發板**。（從原始碼：`make board-install BOARD=<ssh-host>`；若是 64 位元的 Raspberry Pi，請加上 `BOARD_TARGET=aarch64-unknown-linux-musl`。）
2. 把開發板的 **OTG** USB 連接埠接到底座的 USB 連接埠。
3. 把開發板的 Ethernet 接到電腦（或讓兩者加入同一個私人網路）。
4. 如果 Bridge 尚未執行，請啟動它：`ssh <ssh-host> /opt/ns2bench/ns2bench-hidd.sh start --bind <board-ip>`。
5. 確認電腦上的權杖檔案 `~/.ns2bench/hidd.token` 與開發板上的權杖相同。
6. 在**設定 → 裝置**中設定：
   - **輸入連線：** 網路 Bridge
   - **Bridge 位址：** 開發板的 IP（預設 `192.168.79.2`）
   - **Bridge 連接埠：** `7631`
   - **Bridge 權杖檔案：** `~/.ns2bench/hidd.token`
7. 按一下**套用輸入連線**。

**確認是否正常運作**

- **Bridge** 狀態標籤變成綠色，並顯示往返時間和 *usb configured*。
- 按下畫面上 Joy-Con 的按鈕。主機會有反應，按鈕也會亮起。

**主機進入睡眠時**

由主機 USB 供電的開發板，會在主機睡眠時斷電，並在主機喚醒時重新開機。請讓 NS2Bench 保持執行：它大約在三秒內就會察覺並自動重新連線。等開發板開機完成、主機設定好 USB 後，輸入就會恢復正常。

為此，開發板必須在開機時自行啟動服務。請為你的開發板加入一個開機腳本，先執行 `/opt/ns2bench/gadget.sh`，再執行 `/opt/ns2bench/ns2bench-hidd.sh start`。請用你的開發板和底座實際測試一次睡眠與喚醒。

**常見問題**

- *Bridge 離線 · 重新連線中：* 檢查線材和開發板的 IP。Ping 一下開發板，確認 Bridge 正在上面執行。
- *Bridge 已連線，但 usb 未設定：* 喚醒主機。確認線材接的是開發板的 **OTG** 連接埠，而不是只供電的連接埠。有些線材只能供電。
- *在 HOME 選單中滑鼠沒有反應：* 這是正常的。主機會讀取滑鼠，但只有部分軟體會顯示游標。
- *鍵盤沒有反應：* 主機的 USB 鍵盤支援位於「系統設定 → 控制器與配件」中。
- 在開發板上，只能用 `ns2bench-hidd.sh` 和 `gadget.sh` 來停止和啟動服務。不要依名稱模式結束程序。

## Raspberry Pi Pico

![Raspberry Pi Pico：USB 接主機，UART 透過 USB 轉接器接電腦](input-pico.svg)

一塊執行 NS2Bench Bridge 韌體的 Raspberry Pi Pico（RP2040）。它的 USB 連接埠接到主機，呈現與標準模式 Linux 開發板相同的控制器 + 鍵盤 + 滑鼠。電腦則透過另一個 USB 轉 UART 轉接器，以序列（UART）連線和它通訊。

> Pico 韌體已可建置，但尚未在實際硬體上執行過。歡迎回報測試結果。

**所需零件**

- Raspberry Pi Pico（RP2040）和一條 micro-USB 線
- 一個 **3.3 V** USB 轉 UART 轉接器（CP2102、FT232 或 CH340）
- 三條杜邦線
- 上述的擷取卡設定

**步驟**

1. 從應用程式燒錄 Pico：**設定 → 安裝 → 燒錄 Pico**，然後按住 **BOOTSEL** 按鈕，同時把 Pico 插到**電腦**上。應用程式會把韌體複製到 **RPI-RP2** 磁碟，Pico 隨即重新啟動。然後拔下它。（從原始碼：`make pico` 會建置 `firmware/pico/ns2bench-pico.uf2`；請自行把它複製到 RPI-RP2 磁碟。）
2. 把 Pico 接到轉接器：
   - Pico **GP0**（第 1 腳，UART0 TX）→ 轉接器 **RX**
   - Pico **GP1**（第 2 腳，UART0 RX）← 轉接器 **TX**
   - Pico **GND**（第 3 腳）— 轉接器 **GND**
   - 轉接器的 3.3 V / 5 V 腳位不要接。Pico 由主機供電。
3. 把 Pico 的 micro-USB 接到底座的 USB 連接埠。
4. 把轉接器插到電腦上。
5. 在**設定 → 裝置**中設定：
   - **輸入連線：** 序列埠 Bridge
   - **序列埠：** 轉接器（例如 macOS 上的 `/dev/cu.usbserial-XXXX`、Linux 上的 `/dev/ttyUSB0`、Windows 上的 `COM3`）
   - **鮑率：** `921600`
6. 按一下**套用輸入連線**。

選用：以 `NS2BENCH_TOKEN=... make pico` 建置，可讓 Pico 要求權杖（在電腦上設定相同的權杖檔案）。如果你的轉接器無法使用 921600，請以 `NS2BENCH_BAUD=115200` 建置。

**確認是否正常運作**

- **Bridge** 狀態標籤變成綠色，並顯示 *usb configured*。
- 按下畫面上的 Joy-Con 按鈕，主機能收到。

**常見問題**

- *沒有出現序列埠：* 安裝轉接器的驅動程式（CP210x、FTDI 或 CH340），然後重新插拔。
- *Bridge 離線：* 很可能是 TX 和 RX 接反了。TX 一定要接 RX。也請檢查 GND。
- *回應亂碼或沒有回應：* 鮑率必須和韌體一致（除非你用其他值建置，否則是 921600）。
- *絕對不要使用 5 V 轉接器。* Pico 的腳位只能承受 3.3 V。

## CH9329 模組

![CH9329：USB 接主機，UART 接電腦](input-ch9329.svg)

CH9329 模組會把序列指令轉換成 USB 鍵盤和滑鼠。它便宜又簡單，但**沒有控制器功能**：沒有按鈕，也沒有搖桿。只適合用來打字和指向。

**所需零件**

- 一個 CH9329 模組
- 一個 3.3 V USB 轉 UART 轉接器（除非你的模組線材內建 USB 序列功能）
- 上述的擷取卡設定

**步驟**

1. 把模組的 USB（HID）端插到底座的 USB 連接埠。
2. 把 UART 端接到轉接器：模組 TX → 轉接器 RX，模組 RX ← 轉接器 TX，GND — GND。（如果模組內建 USB 序列功能，可略過此步驟。）
3. 把轉接器（或模組的序列線）插到電腦上。
4. 在**設定 → 裝置**中設定：
   - **輸入連線：** CH9329
   - **序列埠：** 轉接器的連接埠
   - **鮑率：** 模組的設定值，通常是 `9600`
5. 按一下**套用輸入連線**。

**確認是否正常運作**

- **Bridge** 狀態標籤變成綠色。
- 在原生鍵盤模式中打字，內容會出現在主機上（例如文字欄位中）。

**常見問題**

- *控制器按鈕沒有反應：* 這是預期中的。CH9329 只支援鍵盤和滑鼠。
- *完全沒有反應：* 檢查鮑率，必要時把 TX/RX 對調。
- *主機不理會鍵盤：* 檢查「系統設定 → 控制器與配件 → USB 鍵盤」。

## 選用：離線主機網路

![讓主機使用離線網路的三種方式](network.svg)

只有在使用本地網路工具（例如自製軟體載入器）時才需要這個。擷取和輸入永遠不需要網路。

無論你選擇哪一種方式，主機都**不會連上網際網路**。NS2Bench 絕不轉送流量，絕不開啟網際網路共享、NAT 或代理伺服器，也絕不查詢外部 DNS 伺服器。它的本地 DNS 只回應主機的連線測試；其他名稱一律拒絕。主機嘗試的每一次查詢都會顯示在**網路**記錄中。

電腦與 Linux 開發板之間的 Bridge 連線是另外一回事，可以使用你平常的區域網路。

### A · 直接接線（macOS 或 Linux）

**所需零件：** 電腦上一個專供主機使用的備用 Ethernet 轉接器、一條 Ethernet 線，以及底座的 LAN 連接埠（或主機上的 USB-Ethernet 轉接器）。

1. 為這個專用轉接器設定固定的私人位址，例如 `192.168.80.1/24`。NS2Bench 不會替你設定網路介面。
2. 在 `bench.json` 中描述該介面（使用你的轉接器介面名稱）：
   ```json
   "network": { "interface": "en7", "address": "192.168.80.1", "subnet": "192.168.80.0/24",
                "pool_start": "192.168.80.10", "pool_end": "192.168.80.13" }
   ```
   建置網路服務：`make netd`。
3. 在不開啟任何連接埠的情況下檢查設定：`native/target/release/ns2bench-netd --check bench.json`。
4. 將它安裝為服務：
   - **macOS：** 把執行檔複製到 `~/.ns2bench/bin/ns2bench-netd`，填寫 `launchd/ns2bench-netd.plist.template` 並複製到 `~/Library/LaunchAgents/`。不需要 root 權限。
   - **Linux：** 安裝到 `/opt/ns2bench/ns2bench-netd`，並使用 systemd 單元 `launchd/ns2bench-netd.service`。
5. 把主機的 Ethernet 接到專用轉接器。
6. 在**設定 → 網路**中，選擇**直連、隔離的電腦 ↔ 主機**。視需要開啟本地 DHCP、DNS 和連線測試頁面，然後套用。

主機會從位址池取得一個位址（例如 `192.168.80.10`–`192.168.80.13`）。**網路**分頁會顯示即時狀態和租用資訊。

如果 IP 轉送已開啟，或服務無法檢查這一點，它就會拒絕啟動。這是刻意的設計。

### B · 隔離的路由器、VLAN 或 Wi-Fi（任何作業系統）

1. 為主機設定一個路由器、VLAN 或 Wi-Fi 網路，並**封鎖網際網路（包括 IPv6）**。一般的家用區域網路並不是隔離的。
2. 讓路由器分配位址（DHCP）。
3. 用 Ethernet 或 Wi-Fi 把主機連上該網路。
4. 在**設定 → 網路**中，選擇**隔離的路由器 / VLAN / Wi-Fi**。這會關閉 NS2Bench 的本地 DHCP 和 DNS，避免與路由器衝突。

### C · Windows 電腦搭配遠端輔助裝置

Windows 無法在本機執行網路服務。請改在 Linux 開發板或電腦上執行，再從 Windows 控制它。

1. 在 Linux 輔助裝置上，依照方式 A 設定 `ns2bench-netd`。由輔助裝置負責主機的 Ethernet。
2. 產生權杖：`openssl rand -hex 16 > ~/.ns2bench/netd-control.token`。把同一個檔案複製到 Windows 電腦。
3. 在輔助裝置的設定中，於一個**不在**主機網路上的私人位址新增管理監聽：
   ```json
   "network": { "control": { "listen": "192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
4. 在 Windows 上的 NS2Bench 設定中，指向輔助裝置：
   ```json
   "network": { "helper": { "url": "http://192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
5. 讓管理連線保持私密：使用直接接線或開發板的 USB 網路。它的流量是未加密的 HTTP。

之後，Windows 上的**網路**分頁就會顯示輔助裝置的狀態和控制項。你也可以不用輔助裝置，改用方式 B。

**常見問題**

- *網路分頁顯示無法使用：* 服務沒有在執行，或服務和 UI 使用了不同的狀態資料夾。在 Windows 上，請檢查 `network.helper`，並確認兩個權杖檔案一致。
- *主機取得不到位址：* 檢查設定中的介面名稱和子網路，並確認線材接的是專用轉接器。
- *主機顯示沒有網際網路：* 這是正確的。連線測試會在本地通過，但不會再往外連。

## 代理

狀態標籤都變成綠色後，代理就能使用測試台。開啟**設定 → API 與 MCP**，可以看到 API 位址、請求範例，以及可直接複製給 Claude Code、Codex 或 opencode 使用的 MCP 設定。代理和使用者共用同一組控制項，而「僅手動」開關隨時都能阻擋代理的輸入。

## Windows 注意事項

- 擷取使用 `ffmpeg` 搭配 DirectShow。請關閉其他佔用擷取卡的應用程式。
- Linux 開發板、Pico 和 CH9329 都能在 Windows 上使用。序列埠名稱為 `COM3`、`COM4` 等。
- Windows 上沒有本地網路服務。請使用遠端輔助裝置（方式 C）或隔離的路由器（方式 B）。
