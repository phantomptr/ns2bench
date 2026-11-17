# NS2Bench einrichten

Mit NS2Bench kannst du (oder ein KI-Agent) eine Nintendo Switch 2 vom Computer aus sehen, hören und steuern.
Diese Anleitung zeigt, wie du alles verkabelst.

![NS2Bench-Aufbau auf einen Blick](overview.svg)

Jeder Aufbau besteht aus zwei unabhängigen Teilen:

- **Augen (immer):** Der HDMI-Ausgang der Konsole geht in eine USB-Capture-Karte. Die Karte liefert dem Computer Bild und Ton.
- **Hände (optional):** Ein kleines Eingabegerät steckt im USB-Port der Konsole. Der Computer schickt ihm Befehle. Die Konsole sieht einen kabelgebundenen Controller, eine Tastatur und eine Maus.

Menschen nutzen die NS2Bench-Seite im Browser oder die Desktop-App. Agenten nutzen die HTTP-API oder MCP auf demselben Computer (`127.0.0.1:7630`). Alle teilen sich eine Bench, und du siehst jede Agentenaktion live.

**Die Konsole bleibt offline.** NS2Bench gibt ihr nie Internetzugang. Capture und USB-Eingabe brauchen überhaupt kein Netzwerk.

## Welchen Aufbau brauche ich?

| Was du hast | Aufbau | Eingabeverbindung in den Einstellungen |
|---|---|---|
| Nur eine Capture-Karte | [Nur Anzeige](#view-only) | Nur Anzeige |
| Ein Linux-Board mit USB-Device-Modus (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5) | [Linux-Board](#linux-usb-gadget-board) | Netzwerk-Bridge |
| Ein Raspberry Pi Pico (RP2040) und ein 3,3-V-USB-UART-Adapter | [Pico](#raspberry-pi-pico) | Serielle Bridge |
| Ein CH9329-Modul (nur Tastatur und Maus) | [CH9329](#ch9329-module) | CH9329 |
| Du willst die Konsole zusätzlich in einem lokalen Netzwerk (Homebrew-Tools) | [Offline-Netzwerk](#optional-offline-console-network) | (Tab „Netzwerk“) |

Unsicher? Fang mit **Nur Anzeige** an und ergänze später ein Eingabegerät. Eingabeeinstellungen ändern sich ohne Neustart der App.

Wenn du jede Controllertaste willst, auch C, GL und GR, nimm ein Linux-Board. Sein **Pro**-Modus zeigt der Konsole einen Switch 2 Pro Controller. Der Pico und der Standardmodus des Boards decken alle Tasten außer C, GL und GR ab. CH9329 hat gar keinen Controller.

## Bevor du anfängst: dein Computer

Installiere die App aus dem [neuesten Release](https://github.com/phantomptr/ns2bench/releases/latest) und dann:

### macOS (14 oder neuer)

- **Sonst nichts zu installieren.** Capture, Ton und das Lesen von Bildschirmtext sind in die App eingebaut.
- Erster Start: Wenn macOS meldet, Apple konnte die .dmg nicht überprüfen, oder die App sei beschädigt oder könne nicht überprüft werden, führe vor dem Öffnen der .dmg `xattr -dr com.apple.quarantine ~/Downloads/NS2Bench-*.dmg` aus (oder `xattr -dr com.apple.quarantine /Applications/NS2Bench.app`, sobald sie installiert ist), oder klicke in Systemeinstellungen → Datenschutz & Sicherheit auf **Dennoch öffnen**. Nur bei der Erstinstallation: Updates aus der App heraus brauchen das nicht.
- Erlaube **Kamera** (das Bild), **Mikrofon** (Konsolenton) und **Lokales Netzwerk** (das Eingabegerät), wenn danach gefragt wird. Später änderst du das unter Systemeinstellungen → Datenschutz & Sicherheit.
- **Clips** (Clip speichern) brauchen ffmpeg: `brew install ffmpeg`.
- Serielle Adapter (Pico, CH9329) funktionieren ohne Treiber; sie erscheinen als `/dev/cu.usbserial-…` oder `/dev/cu.usbmodem…`.

### Windows 10/11

- **Installiere ffmpeg** (Capture, Ton und Clips nutzen es): `winget install Gyan.FFmpeg`, dann NS2Bench neu starten. NS2Bench findet ffmpeg auch, wenn es mit Chocolatey oder Scoop installiert wurde, oder eine `ffmpeg.exe` neben `ns2bench-app.exe`.
- SmartScreen meldet eventuell „Der Computer wurde durch Windows geschützt“: Wähle **Weitere Informationen → Trotzdem ausführen**.
- USB-UART-Adapter brauchen eventuell den Treiber des Herstellers (CH340/CH341 oder CP210x). Der Port erscheint im Geräte-Manager als `COM3` o. Ä.
- Optional: [Tesseract](https://github.com/UB-Mannheim/tesseract/wiki) zum Lesen von Bildschirmtext.
- Der Netzwerkdienst für die Offline-Konsole läuft nicht unter Windows: Nutze einen [Linux-Helfer oder einen Router](#optional-offline-console-network).

### Linux

- **ffmpeg:** Das `.deb` installiert es für dich. Beim AppImage installierst du es selbst (`sudo apt install ffmpeg`, `sudo dnf install ffmpeg`, `sudo pacman -S ffmpeg`).
- **Berechtigungen:** Füge dich den Gruppen hinzu, denen Capture-Karten und serielle Ports gehören, dann ab- und wieder anmelden:
  `sudo usermod -aG video,dialout $USER` (Arch: `video,uucp`).
- AppImage: `chmod +x NS2Bench-*.AppImage`. Wenn es nicht startet, installiere FUSE 2 (`sudo apt install libfuse2`).
- Optional: `sudo apt install tesseract-ocr` zum Lesen von Bildschirmtext.
- Serielle Ports erscheinen als `/dev/ttyACM0` (Pico) oder `/dev/ttyUSB0` (die meisten Adapter).

## Die Capture-Karte (jeder Aufbau)

- Jede USB-**UVC**-HDMI-Capture-Karte funktioniert. Ziel ist 1080p60. Getestet: UGREEN 15389 (MS2130).
- Verbinde den **HDMI-Ausgang** des Konsolen-Docks mit dem HDMI-Eingang der Karte. Steck die Karte per USB an den Computer.
- Wähle Karte, Auflösung, Bildrate und Qualität im Panel **Video**. Es zeigt nur, was deine Karte anbietet.
- **Ton** kommt ebenfalls von der Capture-Karte. Es gibt kein separates Audiokabel.
- **macOS:** Ein kleiner eingebauter Helfer übernimmt das Capture. Er teilt die Karte mit OBS: Wenn OBS die Karte schon belegt, nutzt NS2Bench das Format von OBS und nimmt ihm die Karte nie weg. Erlaube beim ersten Start **Kamera** (Bild), **Mikrofon** (Konsolenton) und **Lokales Netzwerk** (die Bridge).
- **Linux und Windows:** NS2Bench nutzt `ffmpeg` (zuerst installieren). Die Karte muss eventuell frei sein; schließe also OBS oder andere Apps, die sie belegen, wenn das Bild nicht startet.
- Führe `ns2bench devices` aus, um die Capture-Karten aufzulisten, die NS2Bench sieht.

## Nur Anzeige

![Nur Anzeige: eine Capture-Karte, kein Eingabegerät](view-only.svg)

Die einfachste Bench. Du kannst live zuschauen, aufnehmen, Screenshots in voller Auflösung machen und Bildschirmtext lesen (OCR). Agenten können auch zuschauen, aber niemand kann Tasten drücken.

**Teile**

- Konsole im Dock
- USB-HDMI-Capture-Karte (UVC)
- HDMI-Kabel und das USB-Kabel der Karte

**Schritte**

1. Verbinde den HDMI-Ausgang des Docks mit der Capture-Karte.
2. Steck die Capture-Karte an den Computer.
3. Starte NS2Bench und öffne `http://127.0.0.1:7630` (oder die Desktop-App).
4. Wähle deine Karte im Panel **Video**.
5. Setze unter **Einstellungen → Geräte** die **Eingabeverbindung** auf **Nur Anzeige** und klicke auf **Eingabeverbindung anwenden**.

**Prüfen, ob es funktioniert**

- Die **Video**-Anzeige in der oberen Leiste wird grün und das Konsolenbild erscheint.
- Die **Bridge**-Anzeige sagt *nicht konfiguriert*. Das ist hier normal.

**Häufige Probleme**

- *Schwarzes Bild:* Wecke die Konsole; achte darauf, dass sie im Dock steckt und das Dock Strom hat. Probier ein anderes HDMI-Kabel.
- *Karte fehlt in der Liste:* Unter macOS den Kamerazugriff erlauben. Unter Linux oder Windows jede App schließen, die die Karte benutzt.
- *Kein Ton:* Unter macOS den Mikrofonzugriff erlauben. Prüfe, ob die Karte als Audioquelle ausgewählt ist.

## Linux-USB-Gadget-Board

![Linux-Board: USB zur Konsole, Ethernet zum Computer](input-linux-board.svg)

Ein kleines Linux-Board gibt sich als USB-Geräte aus. Darauf läuft **NS2Bench Bridge** (`ns2bench-board`). Das ist der getestete Aufbau (Luckfox Pico Pro/Max). Er unterstützt beide Controllermodi:

- **Standard** (Voreinstellung): Die Konsole sieht einen kabelgebundenen Controller im HORI-Stil + USB-Tastatur + USB-Maus.
- **Pro**: Die Konsole sieht einen Switch 2 Pro Controller mit allen Tasten, auch C, GL und GR. In diesem Modus gibt es keine Tastatur und Maus.

Wechsle den Modus mit **Die Konsole sieht** im Steuerungs-Panel. Das dauert etwa 8 Sekunden.

**Teile**

- Ein Linux-Board mit USB-Device-Modus (OTG): Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W oder Pi 4/5 über den USB-C-Port
- Ein USB-Kabel vom **OTG**-Port des Boards zum USB-Port des Docks
- Ein Ethernet-Kabel zum Computer (oder Wi-Fi bzw. das USB-Netzwerk des Boards)
- Die Capture-Karte wie oben eingerichtet

**Schritte**

1. Installiere NS2Bench Bridge aus der App auf dem Board: **Einstellungen → Installieren → Bridge-Board**, gib seinen SSH-Host ein, klicke auf **Auf Board installieren**. (Aus dem Quellcode: `make board-install BOARD=<ssh-host>`; für einen 64-Bit-Raspberry-Pi füge `BOARD_TARGET=aarch64-unknown-linux-musl` hinzu.)
2. Steck den **OTG**-USB-Port des Boards in den USB-Port des Docks.
3. Verbinde das Ethernet des Boards mit dem Computer (oder bring beide ins selbe private Netzwerk).
4. Starte die Bridge, falls sie noch nicht läuft: `ssh <ssh-host> /opt/ns2bench/ns2bench-hidd.sh start --bind <board-ip>`.
5. Achte darauf, dass die Token-Datei `~/.ns2bench/hidd.token` auf dem Computer zum Token des Boards passt.
6. Setze unter **Einstellungen → Geräte**:
   - **Eingabeverbindung:** Netzwerk-Bridge
   - **Bridge-Adresse:** die IP des Boards (Standard `192.168.79.2`)
   - **Bridge-Port:** `7631`
   - **Bridge-Tokendatei:** `~/.ns2bench/hidd.token`
7. Klicke auf **Eingabeverbindung anwenden**.

**Prüfen, ob es funktioniert**

- Die **Bridge**-Anzeige wird grün und zeigt die Antwortzeit und *usb configured*.
- Drück eine Taste am Joy-Con auf dem Bildschirm. Die Konsole reagiert und die Taste leuchtet auf.

**Wenn die Konsole schläft**

Ein Board, das über den USB der Konsole Strom bekommt, verliert ihn, wenn die Konsole schläft, und startet neu, wenn sie aufwacht. Lass NS2Bench laufen: Es merkt das innerhalb von etwa drei Sekunden und verbindet sich selbst neu. Die Eingabe funktioniert wieder, sobald das Board hochgefahren ist und die Konsole ihren USB eingerichtet hat.

Dafür muss das Board beim Booten selbst wieder starten. Lege für dein Board ein Boot-Skript an, das `/opt/ns2bench/gadget.sh` und danach `/opt/ns2bench/ns2bench-hidd.sh start` ausführt. Teste einmal echtes Schlafen und Aufwachen mit deinem Board und Dock.

**Häufige Probleme**

- *Bridge offline · reconnecting:* Prüfe Kabel und IP des Boards. Ping das Board. Achte darauf, dass die Bridge darauf läuft.
- *Bridge verbunden, aber USB nicht konfiguriert:* Wecke die Konsole. Prüfe, ob das Kabel im **OTG**-Port des Boards steckt und nicht in einem reinen Strom-Port. Manche Kabel übertragen nur Strom.
- *Maus tut nichts im HOME-Menü:* normal. Die Konsole liest die Maus, aber nur manche Software zeigt einen Zeiger.
- *Tastatur wird ignoriert:* Die USB-Tastaturunterstützung der Konsole findest du unter Systemeinstellungen → Controller und Zubehör.
- Stoppe und starte Dinge auf dem Board nur mit `ns2bench-hidd.sh` und `gadget.sh`. Beende keine Prozesse per Muster.

## Raspberry Pi Pico

![Raspberry Pi Pico: USB zur Konsole, UART zu einem USB-Adapter am Computer](input-pico.svg)

Ein Raspberry Pi Pico (RP2040) mit NS2Bench-Bridge-Firmware. Sein USB-Port geht zur Konsole und zeigt denselben Controller + Tastatur + Maus wie ein Linux-Board im Standardmodus. Der Computer spricht mit ihm über eine serielle (UART-)Verbindung über einen separaten USB-UART-Adapter.

> Die Pico-Firmware ist gebaut, lief aber noch nicht auf echter Hardware. Rückmeldungen sind willkommen.

**Teile**

- Raspberry Pi Pico (RP2040) und ein Micro-USB-Kabel
- Ein **3,3-V**-USB-UART-Adapter (CP2102, FT232 oder CH340)
- Drei Jumper-Kabel
- Die Capture-Karte wie oben eingerichtet

**Schritte**

1. Flashe den Pico aus der App: **Einstellungen → Installieren → Pico flashen**, halte dann die Taste **BOOTSEL** gedrückt, während du den Pico an den **Computer** steckst. Die App kopiert die Firmware auf das Laufwerk **RPI-RP2**, und der Pico startet neu. Zieh ihn ab. (Aus dem Quellcode: `make pico` baut `firmware/pico/ns2bench-pico.uf2`; kopiere sie selbst auf das Laufwerk RPI-RP2.)
2. Verkabele den Pico mit dem Adapter:
   - Pico **GP0** (Pin 1, UART0 TX) → Adapter **RX**
   - Pico **GP1** (Pin 2, UART0 RX) ← Adapter **TX**
   - Pico **GND** (Pin 3) — Adapter **GND**
   - Lass den 3,3-V-/5-V-Pin des Adapters unbelegt. Der Pico bekommt Strom von der Konsole.
3. Steck den Micro-USB des Pico in den USB-Port des Docks.
4. Steck den Adapter an den Computer.
5. Setze unter **Einstellungen → Geräte**:
   - **Eingabeverbindung:** Serielle Bridge
   - **Serieller Port:** der Adapter (zum Beispiel `/dev/cu.usbserial-XXXX` unter macOS, `/dev/ttyUSB0` unter Linux, `COM3` unter Windows)
   - **Baudrate:** `921600`
6. Klicke auf **Eingabeverbindung anwenden**.

Optional: Baue mit `NS2BENCH_TOKEN=... make pico`, damit der Pico ein Token verlangt (lege dieselbe Token-Datei auf dem Computer an). Baue mit `NS2BENCH_BAUD=115200`, wenn dein Adapter keine 921600 schafft.

**Prüfen, ob es funktioniert**

- Die **Bridge**-Anzeige wird grün und zeigt *usb configured*.
- Ein Druck auf den Joy-Con auf dem Bildschirm kommt an der Konsole an.

**Häufige Probleme**

- *Kein serieller Port erscheint:* Installiere den Treiber des Adapters (CP210x, FTDI oder CH340) und steck ihn neu ein.
- *Bridge offline:* TX und RX sind wahrscheinlich vertauscht. TX geht immer an RX. Prüfe auch GND.
- *Wirre oder keine Antworten:* Die Baudrate muss zur Firmware passen (921600, außer du hast mit einem anderen Wert gebaut).
- *Nutze nie einen 5-V-Adapter.* Die Pins des Pico vertragen nur 3,3 V.

## CH9329-Modul

![CH9329: USB zur Konsole, UART zum Computer](input-ch9329.svg)

Ein CH9329-Modul macht aus seriellen Befehlen eine USB-Tastatur und -Maus. Es ist günstig und einfach, **hat aber keinen Controller**: keine Tasten, keine Sticks. Nutze es nur zum Tippen und Zeigen.

**Teile**

- Ein CH9329-Modul
- Ein 3,3-V-USB-UART-Adapter, außer das Kabel deines Moduls hat USB-Seriell eingebaut
- Die Capture-Karte wie oben eingerichtet

**Schritte**

1. Steck die USB-(HID-)Seite des Moduls in den USB-Port des Docks.
2. Verkabele die UART-Seite mit dem Adapter: Modul TX → Adapter RX, Modul RX ← Adapter TX, GND — GND. (Entfällt, wenn das Modul USB-Seriell eingebaut hat.)
3. Steck den Adapter (oder das serielle Kabel des Moduls) an den Computer.
4. Setze unter **Einstellungen → Geräte**:
   - **Eingabeverbindung:** CH9329
   - **Serieller Port:** der Port des Adapters
   - **Baudrate:** die Einstellung des Moduls, meist `9600`
5. Klicke auf **Eingabeverbindung anwenden**.

**Prüfen, ob es funktioniert**

- Die **Bridge**-Anzeige wird grün.
- Tippen im nativen Tastaturmodus erscheint auf der Konsole (zum Beispiel in einem Textfeld).

**Häufige Probleme**

- *Controllertasten tun nichts:* erwartet. CH9329 kann nur Tastatur und Maus.
- *Nichts passiert:* Prüfe die Baudrate und vertausche bei Bedarf TX/RX.
- *Konsole ignoriert die Tastatur:* Prüfe Systemeinstellungen → Controller und Zubehör → USB-Tastatur.

## Optional: Offline-Netzwerk für die Konsole

![Drei Wege, der Konsole ein Offline-Netzwerk zu geben](network.svg)

Das brauchst du nur für lokale Netzwerktools wie Homebrew-Loader. Capture und Eingabe brauchen es nie. Die Varianten A–C nutzen ein eigenes Netzwerk für die Konsole. Variante D lässt sie in deinem Heim-Wi-Fi.

Egal welche Variante du wählst, die Konsole bekommt **kein Internet**. NS2Bench leitet nie Datenverkehr weiter, schaltet nie Internetfreigabe, NAT oder einen Proxy ein und fragt nie einen externen DNS-Server. Sein lokales DNS beantwortet nur den Verbindungstest der Konsole; jeder andere Name wird abgelehnt. Jede Anfrage, die die Konsole versucht, erscheint im **Netzwerk**-Log.

Die Bridge-Verbindung vom Computer zu einem Linux-Board ist davon getrennt. Sie kann dein normales LAN nutzen.

### A · Direktes Kabel (macOS oder Linux)

**Teile:** ein zusätzlicher Ethernet-Adapter am Computer nur für die Konsole, ein Ethernet-Kabel und der LAN-Port des Docks (oder ein USB-Ethernet-Adapter an der Konsole).

1. Gib dem eigenen Adapter eine feste private Adresse, zum Beispiel `192.168.80.1/24`. NS2Bench konfiguriert das Interface nicht für dich.
2. Beschreibe dieses Interface in `bench.json` (nutze den Interface-Namen deines Adapters):
   ```json
   "network": { "interface": "en7", "address": "192.168.80.1", "subnet": "192.168.80.0/24",
                "pool_start": "192.168.80.10", "pool_end": "192.168.80.13" }
   ```
   Baue den Netzwerkdienst: `make netd`.
3. Prüfe die Konfiguration, ohne Ports zu öffnen: `native/target/release/ns2bench-netd --check bench.json`.
4. Installiere ihn als Dienst:
   - **macOS:** Kopiere die Binärdatei nach `~/.ns2bench/bin/ns2bench-netd`, fülle `launchd/ns2bench-netd.plist.template` aus und kopiere sie nach `~/Library/LaunchAgents/`. Kein Root nötig.
   - **Linux:** Installiere ihn unter `/opt/ns2bench/ns2bench-netd` und nutze die systemd-Unit `launchd/ns2bench-netd.service`.
5. Verbinde das Ethernet der Konsole mit dem eigenen Adapter.
6. Wähle unter **Einstellungen → Netzwerk** die Option **Direkt und isoliert: Computer ↔ Konsole**. Schalte lokales DHCP, DNS und die Verbindungstest-Seite nach Bedarf ein und wende an.

Die Konsole bekommt eine Adresse aus dem Pool (zum Beispiel `192.168.80.10`–`192.168.80.13`). Der Tab **Netzwerk** zeigt Live-Status und Leases.

Der Dienst startet nicht, wenn IP-Forwarding an ist oder er das nicht prüfen kann. Das ist Absicht.

### B · Isolierter Router, VLAN oder Wi-Fi (jedes Betriebssystem)

1. Richte für die Konsole einen Router, ein VLAN oder ein Wi-Fi-Netz ein, in dem **Internet gesperrt ist, auch IPv6**. Ein normales Heim-LAN ist nicht isoliert.
2. Lass den Router die Adressen vergeben (DHCP).
3. Verbinde die Konsole per Ethernet oder Wi-Fi damit.
4. Wähle unter **Einstellungen → Netzwerk** die Option **Isolierter Router / VLAN / Wi-Fi**. Damit werden lokales DHCP und DNS von NS2Bench abgeschaltet, damit sie nicht mit dem Router kollidieren.

### C · Windows-Computer mit einem entfernten Helfer

Windows kann den Netzwerkdienst nicht lokal ausführen. Lass ihn stattdessen auf einem Linux-Board oder PC laufen und steuere ihn von Windows aus.

1. Richte auf dem Linux-Helfer `ns2bench-netd` wie in Variante A ein. Der Helfer übernimmt das Ethernet der Konsole.
2. Erzeuge ein Token: `openssl rand -hex 16 > ~/.ns2bench/netd-control.token`. Kopiere dieselbe Datei auf den Windows-Computer.
3. Füge in der Konfiguration des Helfers einen Verwaltungs-Listener auf einer privaten Adresse hinzu, die **nicht** im Konsolennetzwerk liegt:
   ```json
   "network": { "control": { "listen": "192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
4. Verweise in der NS2Bench-Konfiguration unter Windows auf den Helfer:
   ```json
   "network": { "helper": { "url": "http://192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
5. Halte die Verwaltungsverbindung privat: ein direktes Kabel oder das USB-Netzwerk des Boards. Ihr Datenverkehr ist unverschlüsseltes HTTP.

Der Tab **Netzwerk** unter Windows zeigt dann Status und Steuerung des Helfers. Oder lass den Helfer weg und nutze Variante B.

### D · Geteiltes Heimnetz (Wi-Fi, keine zusätzliche Hardware)

![Konsolen im Heim-Wi-Fi ohne Internet: Ihr Gateway ist ihre eigene Adresse, und der Computer beantwortet nur ihren Verbindungstest](shared-network.svg)

Die Konsolen bleiben in deinem normalen Heim-Wi-Fi, neben deinem Computer. Nichts zu kaufen oder anzuschließen. Jede Konsole nutzt **manuelle Einstellungen**, mit denen sie ihren Datenverkehr nirgendwohin schicken kann, und NS2Bench auf deinem Computer beantwortet nur ihren Verbindungstest. So meldet die Konsole trotzdem, dass sie verbunden ist.

**So wird das Internet blockiert**

| Weg nach draußen | Was ihn stoppt |
|---|---|
| Einen Nintendo-Namen nachschlagen (eShop, Updates, Konten, Telemetrie) | Das DNS von NS2Bench lehnt jeden Namen außer dem Verbindungstest ab. Jeder Versuch erscheint im Leck-Wächter und im **Netzwerk**-Log. |
| Per IP-Adresse zu einem Server verbinden | Das Gateway der Konsole ist **ihre eigene Adresse**, also verlässt nichts, was sie sendet, dein Heimnetz, egal an welche Adresse. |
| IPv6 | Dein Router darf kein IPv6 vergeben, oder schalte es aus. |
| Dein Computer leitet Datenverkehr weiter | Die Konsole schickt deinem Computer nie Datenverkehr als Gateway, und der Helfer stoppt, wenn Weiterleitung oder Internetfreigabe eingeschaltet wird. |
| Die Konsole geht zurück auf automatische Einstellungen (Zurücksetzen, Update, jemand ändert sie) | Der **Leck-Wächter** warnt dich innerhalb von etwa 10 Minuten. Richte zusätzlich eine Pause am Router ein (Schritt 7). |

Die letzte Zeile ist das Einzige, was ein geteiltes Netz nicht ausschließen kann: Dort sind die Einstellungen der Konsole selbst das Schloss. Wenn du „offline auch nach dem Zurücksetzen auf Werkseinstellungen“ brauchst, nimm Variante A oder B. Dort gibt es gar keinen Weg ins Internet.

**Du brauchst:** NS2Bench unter macOS oder Linux mit installiertem Netzwerk-Helfer, deinen Computer und die Konsolen im selben Heimnetz und einen Router, der kein IPv6 vergibt (oder mit ausgeschaltetem IPv6).

1. **Reserviere Adressen in deinem Router** (Google Home, die App deines Routers oder seine Webseite): eine für deinen Computer und eine für jede Konsole. Notiere sie, zum Beispiel Computer `192.168.86.199`, Konsolen `192.168.86.251` und `192.168.86.252`.
2. **Installiere den Netzwerk-Helfer**, falls noch nicht geschehen: **Einstellungen → Installieren → Netzwerk-Helfer**.
3. Öffne **Einstellungen → Netzwerk** und wähle **Geteiltes Heimnetz**.
4. **Wähle den Netzwerkadapter**, der im Netz der Konsolen ist. Die Liste zeigt jeden Adapter mit seiner Adresse, zum Beispiel `en10 · Belkin USB-C LAN · 192.168.86.199/24`.
5. Gib die **Konsolenadressen** ein, setze die drei Häkchen (kein IPv6, Computeradresse reserviert, kein anderes gespeichertes Wi-Fi auf den Konsolen) und klicke auf **Speichern und dieses Netzwerk verwenden**. NS2Bench legt eine Sicherung deiner Einstellungsdatei an. Klicke danach auf **Netzwerk-Helfer neu starten**.
6. **Auf jeder Konsole:** Systemeinstellungen → Internet → dein Wi-Fi → **Einstellungen ändern**:
   - IP-Adresseinstellungen: **Manuell**. IP-Adresse: die reservierte Adresse der Konsole. Subnetzmaske: `255.255.255.0`. **Gateway: die eigene Adresse der Konsole.**
   - DNS-Einstellungen: **Manuell**. Primäres und sekundäres DNS: **die Adresse deines Computers**.
   - Speichern und den Verbindungstest ausführen. Er klappt, und die Konsole meldet, dass sie verbunden ist.
   NS2Bench zeigt genau diese Werte für jede Konsole unter **Gib das auf jeder Konsole ein**.
7. **Empfohlenes zweites Schloss:** Pausiere oder sperre in der App deines Routers das Internet für die Konsolen (Google Home: Wi-Fi → Family Wi-Fi → Pausieren). Prüfe, ob der Verbindungstest mit aktiver Pause weiterhin klappt.
8. Lösche jedes andere gespeicherte Wi-Fi-Netz auf jeder Konsole und vermeide lokales Drahtlosspiel mit einer Konsole mit neuerer Firmware (Konsolen können sich so gegenseitig Systemupdates weitergeben).

**Den Leck-Wächter lesen** (Einstellungen → Netzwerk, Prüfung alle 30 Sekunden):

- **Geschützt:** Die Konsole hat NS2Bench vor Kurzem gefragt. Angezeigt werden wann, ihr letzter Verbindungstest und wie viele Namen abgelehnt wurden.
- **In deinem Netzwerk, nutzt NS2Bench aber nicht:** Sie hat seit 10 Minuten nicht gefragt, obwohl sie im Netzwerk ist. Sie ist vielleicht wieder auf automatischen Einstellungen und online. Prüfe ihre Einstellungen. NS2Bench schickt außerdem eine Desktop-Benachrichtigung.
- **Gerade nicht im Netzwerk:** aus, im Ruhemodus oder unterwegs. Nichts zu tun.

**Häufige Probleme**

- *Die Konsole verbindet sich nicht:* Prüfe, ob ihr DNS die Adresse deines Computers ist, ob der Helfer läuft (Einstellungen → Installieren) und ob der in Schritt 4 gewählte Adapter im Netz der Konsolen ist. Nur eingetragene Adressen bekommen Antworten, prüfe also auch die IP der Konsole.
- *Die Konsole lehnt ihre eigene Adresse als Gateway ab:* Nimm stattdessen eine unbenutzte Adresse außerhalb des DHCP-Bereichs des Routers, zum Beispiel `192.168.86.254`. Achte darauf, dass sie niemand nutzt, weder jetzt noch später.
- *Die Adresse deines Computers hat sich geändert:* Die Konsolen können sich nicht verbinden (sie bleiben offline). Reserviere sie im Router und führe die Einrichtung erneut aus.
- *Nur ein Netzwerk gleichzeitig:* Der Helfer bedient entweder dieses geteilte Netz oder ein direktes Kabel (Variante A), nicht beides.

**Häufige Probleme**

- *Tab „Netzwerk“ sagt nicht verfügbar:* Der Dienst läuft nicht, oder er und die Oberfläche nutzen verschiedene Statusordner. Unter Windows `network.helper` prüfen und ob beide Token-Dateien übereinstimmen.
- *Konsole bekommt keine Adresse:* Prüfe Interface-Name und Subnetz in der Konfiguration und ob das Kabel im eigenen Adapter steckt.
- *Konsole meldet kein Internet:* Das ist richtig. Der Verbindungstest klappt lokal, aber weiter geht nichts.

## Agenten

Sobald die Anzeigen grün sind, können Agenten die Bench nutzen. Öffne **Einstellungen → API & MCP** für die API-Adresse, Anfragebeispiele und eine fertige MCP-Konfiguration zum Kopieren für Claude Code, Codex oder opencode. Agenten und Menschen teilen sich dieselben Bedienelemente, und der Nur-manuell-Schalter sperrt Agenteneingaben jederzeit.

## Hinweise zu Windows

- Capture nutzt `ffmpeg` mit DirectShow. Schließe andere Apps, die die Karte belegen.
- Linux-Board, Pico und CH9329 funktionieren alle unter Windows. Serielle Ports heißen `COM3`, `COM4` usw.
- Unter Windows gibt es keinen lokalen Netzwerkdienst. Nutze einen entfernten Helfer (Variante C) oder einen isolierten Router (Variante B).
