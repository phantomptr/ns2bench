# Allgemein

## Was ist NS2Bench?

Ein Prüfstand für eine Nintendo Switch 2 auf deinem Schreibtisch. Er zeigt Bild und Ton der Konsole auf deinem Computer und lässt dich oder einen KI-Agenten ihre Tasten drücken. Du kannst Clips aufnehmen, Text auf dem Bildschirm lesen und wiederholbare Tests ausführen.

## Für wen ist das?

Für Entwickler, die Software auf echter Hardware testen, für Leute, die wiederkehrende Prüfungen automatisieren, für Barrierefreiheits-Experimente und für alle, die einen KI-Agenten eine Konsole bedienen lassen und dabei zusehen wollen.

## Verändert es meine Konsole?

Nein. NS2Bench macht nur, was ein Fernseher und ein kabelgebundener Controller machen: Es empfängt HDMI und drückt Tasten über USB. Auf der Konsole wird nichts installiert.

## Geht die Konsole online?

Nein. NS2Bench gibt der Konsole nie Internetzugang. Aufnahme und USB-Eingabe brauchen überhaupt kein Netzwerk. Das optionale Konsolennetzwerk beantwortet nur den Verbindungstest der Konsole und leitet nie Datenverkehr weiter. Siehe [Offline-Netzwerk für die Konsole](#offline-console-network).

## Ist es kostenlos?

Ja. NS2Bench ist kostenlos und Open Source unter der GPL-3.0. Du darfst es nutzen, ändern und weitergeben. Wenn du es weitergibst, ob geändert oder nicht, behalte den Hinweis „NS2Bench, created by PhantomPtr“ und die Lizenz bei; bei einer geänderten Version gib den Empfängern auch den Quellcode und kennzeichne sie als geändert (siehe NOTICE). Wenn es dir hilft, kannst du es [unterstützen](README.md#support).

## Gehört es zu Nintendo?

Nein. NS2Bench ist ein unabhängiges Projekt. Nintendo Switch ist eine Marke von Nintendo.

# Einrichtung

## Was brauche ich?

Eine Capture-Karte für Bild und Ton (immer) und ein Eingabegerät, wenn du Tasten drücken willst:

- ein Linux-Board mit USB-Device-Port (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5),
- oder einen Raspberry Pi Pico plus einen 3,3-V-USB-UART-Adapter,
- oder ein CH9329-Modul (nur Tastatur und Maus).

Die [Einrichtungsanleitung](docs/setup/README.md) hat für jede Variante ein Schaltbild und die Schritte.

## Was muss ich auf meinem Computer installieren?

- **macOS:** nichts; erlaube nur Kamera, Mikrofon und Lokales Netzwerk. `brew install ffmpeg`, wenn du Clips speichern willst.
- **Windows:** ffmpeg (`winget install Gyan.FFmpeg`) und einen CH340/CP210x-Treiber, falls dein serieller Adapter einen braucht.
- **Linux:** ffmpeg (das `.deb` installiert es) und die Gruppen `video` und `dialout` (`sudo usermod -aG video,dialout $USER`).

Siehe [Bevor du anfängst](docs/setup/README.md#before-you-start-your-computer).

## Welches Eingabegerät soll ich nehmen?

Wenn du alle Tasten willst, auch C, GL und GR, nimm ein Linux-Board: Sein Pro-Modus zeigt der Konsole einen echten Switch 2 Pro Controller. Ein Pico ist die günstigste Controller-Option (alle Tasten außer C, GL und GR). Ein CH9329 bietet nur Tastatur und Maus. Du kannst auch nur mit Anzeige anfangen und später ein Eingabegerät ergänzen.

## Brauche ich das Dock?

Die Konsole braucht einen Weg, HDMI auszugeben, und das ist normalerweise das Dock. Das Eingabegerät steckt in einem der USB-Ports des Docks.

## Geht es auch ohne Eingabegerät?

Ja. Wähle **Nur Anzeige (kein Eingabegerät)** unter Einstellungen → Geräte. Du kannst zusehen, zuhören, Screenshots machen, Clips speichern und Text auf dem Bildschirm lesen.

## Wo stelle ich die Capture-Karte ein?

Im Bereich **Video** rechts im Hauptfenster. Er listet nur die Formate, die deine Karte anbietet.

## Wo stelle ich das Eingabegerät ein?

Unter **Einstellungen → Geräte**. Änderungen gelten ohne Neustart; die alte Verbindung lässt zuerst alle Tasten los.

## Kann die App die Bridge, die Pico-Firmware oder den Netzwerkhelfer installieren?

Ja, unter **Einstellungen → Installieren**. **Pico flashen** spielt die Bridge-Firmware auf einen Raspberry Pi Pico (halte beim Einstecken BOOTSEL gedrückt). **Auf Board installieren** richtet die Bridge per SSH auf einem Linux-Board ein oder aktualisiert sie (richte vorher die Anmeldung per Schlüssel ein). **Helfer installieren** lässt den Offline-Netzwerkhelfer unter macOS oder Linux als Dienst laufen. Downloads stammen aus dem Release, das zur App passt, und werden vor der Nutzung geprüft.

## Woran erkenne ich, dass es funktioniert?

Die **Video**-Anzeige in der oberen Leiste wird grün und zeigt die Bildrate, und die **Bridge**-Anzeige wird grün, wenn die Konsole das Eingabegerät angenommen hat. **Diagnose** in der oberen Leiste kopiert eine vollständige Statusübersicht.

# Aufnahme und Ton

## Welche Capture-Karten funktionieren?

Jede USB-(UVC-)HDMI-Capture-Karte. Ziel ist 1080p60. Getestet: UGREEN 15389 (MS2130), bei 1080p60 und 1440p30.

## Kann ich gleichzeitig OBS nutzen?

Unter macOS ja: NS2Bench teilt die Karte mit OBS und nimmt sie ihm nie weg. Unter Windows und Linux braucht ffmpeg die Karte meist für sich allein, also schließe andere Apps, die sie belegen.

## Es gibt kein Bild.

Prüfe, ob die Konsole im Dock steckt und eingeschaltet ist, ob das HDMI-Kabel im **Eingang** der Karte steckt und ob im Video-Bereich die richtige Karte ausgewählt ist. Unter macOS erlaube **Kamera** für NS2Bench in Systemeinstellungen → Datenschutz & Sicherheit. Starte **Diagnose** für Details.

## Es gibt keinen Ton.

Klicke in der oberen Leiste auf **Ton aus**, um den Ton einzuschalten. Unter macOS erlaube **Mikrofon** für NS2Bench. Wenn du zwei gleiche Capture-Karten hast, ordnet NS2Bench den Ton anhand der USB-Position dem Bild zu.

## Wie viel CPU braucht die Aufnahme?

Unter macOS etwa 12 % eines Kerns bei 1080p60 (Hardware-JPEG und -H.264). Unter Linux werden MJPEG-Karten ohne Neukodierung durchgereicht.

# Eingabegeräte

## Die Bridge-Anzeige bleibt rot.

- **Linux-Board:** Prüfe, ob das Board Strom hat und unter der Adresse in Einstellungen → Geräte erreichbar ist und ob die Token-Datei zum Token des Boards passt.
- **Pico:** Prüfe den seriellen Port und dass die Baudrate 921600 ist; TX und RX müssen gekreuzt sein (Adapter-TX → Pico GP1, Adapter-RX → Pico GP0) und GND muss verbunden sein.
- **CH9329:** Nimm die Baudrate des Moduls (meist 9600).

Prüfe dann, ob das Eingabegerät im USB-Port der Konsole steckt und die Konsole wach ist.

## Tastendrücke kommen nicht an.

Die Switch 2 braucht einen Tastendruck von mindestens etwa 50 ms. NS2Bench hält jeden Druck mindestens 75 ms. Wenn Bridge grün ist, aber nichts passiert, schläft die Konsole vielleicht oder zeigt einen abgedunkelten Bildschirm: Der erste Druck weckt sie nur auf.

## Wie bekomme ich C, GL und GR?

GL und GR sind die zwei Rückseitentasten des Nintendo Switch 2 Pro Controllers; Joy-Con 2 haben sie nicht. Um sie (und C) zu nutzen, nimm ein Linux-Board und wähle **Die Konsole sieht → Pro Controller** im Bereich Steuerung aktiv. Die Konsole sieht dann einen Switch 2 Pro Controller. Sonst dient C als GameChat-Kurzbefehl.

## Kann ich mein eigenes Gamepad nutzen?

Ja. Schließe es am Computer an; NS2Bench ordnet die Tasten nach Position zu (die untere Aktionstaste ist B).

## Kann ich mit einer Tastatur tippen?

Wähle **Deine Tastatur sendet → Tastatur & Maus** im Bereich Steuerung aktiv. Die Konsole sieht eine echte USB-Tastatur. Dafür brauchst du ein Linux-Board, einen Pico im Standardmodus oder ein CH9329.

## Was passiert, wenn die Konsole schläft?

Ein Board, das über den USB der Konsole versorgt wird, verliert Strom und startet neu, wenn die Konsole aufwacht. NS2Bench verbindet sich von selbst neu. Tastendrücke, während Bridge offline ist, werden verworfen und nie später nachgeholt.

## Kann NS2Bench die Konsole aus dem Standby wecken?

Noch nicht. Die Switch 2 hat kein Wake-on-LAN, und ein Board, das über den USB-Port der Konsole versorgt wird, schaltet sich mit der Konsole ab (getestet: Die Bridge fällt etwa 7 Sekunden nach dem Standby-Modus weg; wenn du die Konsole weckst, startet das Board und NS2Bench verbindet sich von selbst neu, bereit nach etwa 30 Sekunden). Zwei Wege werden untersucht: das Board separat mit Strom versorgen (ein USB-Stromsplitter, der keinen Strom in die Konsole zurückspeist), damit es wach bleibt, oder HDMI-CEC „An TV-Betriebszustand anpassen“, bei dem ein CEC-Gerät auf der HDMI-Seite die Konsole weckt. Bis dahin weck sie mit ihrem Power-Knopf. Wenn du die Konsole aufweckst, startet das Board von selbst und NS2Bench verbindet sich in etwa 30 Sekunden wieder.

# Offline-Netzwerk für die Konsole

## Brauche ich ein Netzwerk?

Nein. Aufnahme und USB-Eingabe funktionieren ohne Netzwerk. Das Konsolennetzwerk ist nur für Tools, die über ein LAN mit der Konsole sprechen.

## Wie gebe ich der Konsole ein LAN ohne Internet?

Verbinde das Ethernet der Konsole (Dock-Port oder USB-Adapter) mit einem eigenen Ethernet-Adapter an deinem Mac oder Linux-Computer und starte `ns2bench-netd`. Es verteilt Adressen und beantwortet nur den Verbindungstest. Oder nutze einen Router, ein VLAN oder ein WLAN mit gesperrtem Internet. Siehe die [Einrichtungsanleitung](docs/setup/README.md#optional-offline-console-network).

## Kann ich die Internetsperre abschalten?

Nein. Das ist keine Einstellung. NS2Bench leitet Konsolenverkehr nie weiter, macht kein NAT und keinen Proxy und fragt nie einen vorgelagerten DNS-Server.

## Ich nutze Windows.

Der Netzwerkdienst läuft unter macOS und Linux. Unter Windows lässt du ihn auf einem Linux-Board oder PC laufen, der den Ethernet-Port der Konsole übernimmt, und steuerst ihn über den Tab Netzwerk ([Remote-Helfer](docs/REFERENCE.md#remote-helper-windows-hosts)), oder du nutzt einen Router mit gesperrtem Internet.

# Agenten und API

## Welche Agenten funktionieren?

Jeder MCP-Client: Claude Code, Codex, opencode und andere. Einstellungen → API & MCP zeigt eine Konfiguration zum Kopieren.

## Kann ich einen Agenten stoppen?

Ja. **Agenten sperren (nur manuell)** im Bereich Steuerung aktiv stoppt Agenten-Eingaben sofort. Nur du kannst das im NS2Bench-Fenster wieder abschalten.

## Ist die API in meinem Netzwerk sicher?

Die Engine lauscht standardmäßig nur auf `127.0.0.1`. Für eine andere Adresse ist ein Token nötig. Webseiten anderer Sites können ihr keine Befehle senden.

## Wo ist die API dokumentiert?

In [docs/REFERENCE.md](docs/REFERENCE.md#agents-mcp-and-http-api).

# App und Updates

## Wie funktionieren Updates?

Die App prüft täglich die GitHub-Releases und zeigt an, wenn ein Update verfügbar ist. Jeder Download wird vor der Installation mit dem SHA-256 des Releases abgeglichen. Automatischer Download und Installation sind standardmäßig aus; schalte sie unter Einstellungen → Updates ein.

## macOS sagt, die App sei beschädigt oder könne nicht geprüft werden.

Die App ist noch nicht notarisiert. Führe einmal `xattr -dr com.apple.quarantine /Applications/NS2Bench.app` aus und öffne sie dann.

## Windows sagt, es habe meinen PC geschützt.

Wähle **Weitere Informationen → Trotzdem ausführen**. Der Installer ist noch nicht mit einem kostenpflichtigen Zertifikat signiert.

## macOS fragt nach einem Update erneut nach Kamera oder Mikrofon.

Ab 0.0.3 trägt jede NS2Bench-Version dieselbe Signatur, daher fragt macOS einmal und merkt es sich, auch über Neustarts und Updates hinweg. Beim Update von 0.0.2 oder älter fragt es ein letztes Mal.

## Wie ändere ich Design oder Sprache?

Einstellungen → Darstellung. Der Mond-Button in der oberen Leiste wechselt zwischen hell und dunkel.

## Wo sind meine Clips, Screenshots und Logs?

In `~/.ns2bench/`: Clips, Testberichte und eine Logdatei pro Tag in `~/.ns2bench/logs` (7 Tage aufbewahrt). Screenshots landen in deinem Downloads-Ordner. Nichts wird irgendwohin hochgeladen.

## Kann NS2Bench mich benachrichtigen?

Ja. **Einstellungen → Benachrichtigungen** schaltet Desktop-Benachrichtigungen ein, wenn die Bridge oder das Bild wegfällt, Tests und Clips fertig sind, ein Agent zu steuern beginnt oder ein Update bereitsteht. Standardmäßig erscheinen sie nur, während NS2Bench im Hintergrund ist.

## Wie installiere ich das Befehlszeilentool?

**Einstellungen → Installieren → Befehlszeilentool installieren** legt `ns2bench` in deinen PATH (macOS fragt eventuell einmal nach deinem Passwort; das Linux-`.deb` installiert es bereits). Führe danach `ns2bench --help` aus. Es kann alles, was API und MCP können.

# Fehlerbehebung

## Wie melde ich einen Fehler?

Klicke in der oberen Leiste auf **Hilfe**, dann auf **Fehlerbericht speichern** (ein Zip mit Logs, Status und einem Screenshot; keine Tokens), und hänge es an ein [GitHub-Issue](https://github.com/phantomptr/ns2bench/issues/new/choose) an.

## Was bedeuten die Farben im Log?

Blaue Zeilen sind normale Aktivität, orange Zeilen sind Warnungen, die sich von selbst erledigen können, und rote Zeilen brauchen dich. Bei Problemen, die NS2Bench erkennt, sagt ein blauer Kasten unter der Zeile, was passiert ist und was zu tun ist. **Probleme** im Logs-Bereich zeigt nur Warnungen und Fehler; **Alles** fügt Details wie jeden API-Aufruf hinzu.

## Was macht doctor?

**Hilfe → Prüfung starten** in der App (oder `ns2bench doctor`) prüft Video, das Konsolenbild, OCR, die Bridge, den USB-Zustand, die Kalibrierung und die Netzwerkisolation und sagt dir, welcher Teil kaputt ist und was zu tun ist. `--probe` drückt außerdem RIGHT und LEFT im HOME-Menü, um die Eingabe von Anfang bis Ende zu testen.

## Wo bekomme ich Hilfe?

Auf [Discord](https://discord.gg/fzK3xddtrM).
