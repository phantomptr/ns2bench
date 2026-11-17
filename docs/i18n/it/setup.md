# Configurare NS2Bench

NS2Bench permette a te, o a un agente AI, di vedere, sentire e controllare una Nintendo Switch 2 da un computer.
Questa guida spiega come collegare tutto.

![Configurazione di NS2Bench in sintesi](overview.svg)

Ogni configurazione ha due parti indipendenti:

- **Occhi (sempre):** l'uscita HDMI della console va in una scheda di acquisizione USB. La scheda fornisce al computer video e audio.
- **Mani (opzionali):** un piccolo dispositivo di input si collega alla porta USB della console. Il computer gli invia i comandi. La console vede un controller cablato, una tastiera e un mouse.

Le persone usano la pagina di NS2Bench in un browser oppure l'app desktop. Gli agenti usano la HTTP API o MCP sullo stesso computer (`127.0.0.1:7630`). Tutti condividono un unico banco, e vedi ogni azione degli agenti in tempo reale.

**La console resta offline.** NS2Bench non le dà mai accesso a internet. Acquisizione e input USB non richiedono alcuna rete.

## Quale configurazione mi serve?

| Cosa hai | Configurazione | Connessione di input nelle Impostazioni |
|---|---|---|
| Solo una scheda di acquisizione | [Sola visualizzazione](#view-only) | Sola visualizzazione |
| Una scheda Linux con modalità dispositivo USB (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5) | [Scheda Linux](#linux-usb-gadget-board) | Bridge di rete |
| Un Raspberry Pi Pico (RP2040) e un adattatore USB-UART a 3,3 V | [Pico](#raspberry-pi-pico) | Bridge seriale |
| Un modulo CH9329 (solo tastiera e mouse) | [CH9329](#ch9329-module) | CH9329 |
| Vuoi anche la console su una rete locale (strumenti homebrew) | [Rete offline](#optional-offline-console-network) | (scheda Rete) |

Non sei sicuro? Inizia con la **sola visualizzazione**, poi aggiungi un dispositivo di input in seguito. Le impostazioni di input si cambiano senza riavviare l'app.

Se vuoi tutti i pulsanti del controller, compresi C, GL e GR, usa una scheda Linux. La sua modalità **Pro** presenta alla console un Controller Pro per Switch 2. Il Pico e la modalità standard della scheda coprono tutti i pulsanti tranne C, GL e GR. CH9329 non ha alcun controller.

## Prima di iniziare: il tuo computer

Installa l'app dall'[ultima release](https://github.com/phantomptr/ns2bench/releases/latest), poi:

### macOS (14 o successivo)

- **Non serve installare altro.** Acquisizione, audio e lettura del testo a schermo sono integrati nell'app.
- Primo avvio: se macOS dice che Apple non è riuscita a verificare il .dmg, o che l'app è danneggiata o non può essere verificata, esegui `xattr -dr com.apple.quarantine ~/Downloads/NS2Bench-*.dmg` prima di aprire il .dmg (oppure `xattr -dr com.apple.quarantine /Applications/NS2Bench.app` dopo l'installazione), oppure fai clic su **Apri comunque** in Impostazioni di Sistema → Privacy e sicurezza. Solo alla prima installazione: gli aggiornamenti dall'app non ne hanno bisogno.
- Quando richiesto, consenti **Fotocamera** (l'immagine), **Microfono** (l'audio della console) e **Rete locale** (il dispositivo di input). Puoi cambiarli in seguito in Impostazioni di Sistema → Privacy e sicurezza.
- Le **clip** (Salva clip) richiedono ffmpeg: `brew install ffmpeg`.
- Gli adattatori seriali (Pico, CH9329) funzionano senza driver; compaiono come `/dev/cu.usbserial-…` o `/dev/cu.usbmodem…`.

### Windows 10/11

- **Installa ffmpeg** (lo usano acquisizione, audio e clip): `winget install Gyan.FFmpeg`, poi riavvia NS2Bench. NS2Bench trova anche ffmpeg installato con Chocolatey o Scoop, oppure un `ffmpeg.exe` accanto a `ns2bench-app.exe`.
- SmartScreen potrebbe dire "Windows ha protetto il PC": scegli **Ulteriori informazioni → Esegui comunque**.
- Gli adattatori USB-UART potrebbero richiedere il driver del produttore (CH340/CH341 o CP210x). La porta compare come `COM3` o simile in Gestione dispositivi.
- Opzionale: [Tesseract](https://github.com/UB-Mannheim/tesseract/wiki) per la lettura del testo a schermo.
- Il servizio di rete offline per la console non funziona su Windows: usa un [helper Linux o un router](#optional-offline-console-network).

### Linux

- **ffmpeg:** il `.deb` lo installa per te. Con l'AppImage, installalo tu (`sudo apt install ffmpeg`, `sudo dnf install ffmpeg`, `sudo pacman -S ffmpeg`).
- **Permessi:** aggiungiti ai gruppi proprietari delle schede di acquisizione e delle porte seriali, poi esci e rientra:
  `sudo usermod -aG video,dialout $USER` (Arch: `video,uucp`).
- AppImage: `chmod +x NS2Bench-*.AppImage`. Se non si avvia, installa FUSE 2 (`sudo apt install libfuse2`).
- Opzionale: `sudo apt install tesseract-ocr` per la lettura del testo a schermo.
- Le porte seriali compaiono come `/dev/ttyACM0` (Pico) o `/dev/ttyUSB0` (la maggior parte degli adattatori).

## La scheda di acquisizione (ogni configurazione)

- Va bene qualsiasi scheda di acquisizione HDMI USB **UVC**. L'obiettivo è 1080p60. Testata: UGREEN 15389 (MS2130).
- Collega l'**uscita HDMI** della base della console all'ingresso HDMI della scheda. Collega la scheda al computer via USB.
- Scegli scheda, risoluzione, frame rate e qualità nel pannello **Video**. Elenca solo ciò che la tua scheda offre.
- Anche l'**audio** arriva dalla scheda di acquisizione. Non c'è un cavo audio separato.
- **macOS:** un piccolo helper integrato si occupa dell'acquisizione. Condivide la scheda con OBS: se OBS sta già usando la scheda, NS2Bench usa il formato di OBS e non gliela toglie mai. Al primo avvio, consenti **Fotocamera** (immagine), **Microfono** (audio della console) e **Rete locale** (il Bridge).
- **Linux e Windows:** NS2Bench usa `ffmpeg` (installalo prima). La scheda potrebbe dover essere libera, quindi chiudi OBS o altre app che la tengono occupata se l'immagine non parte.
- Esegui `ns2bench devices` per elencare le schede di acquisizione che NS2Bench vede.

## Sola visualizzazione

![Sola visualizzazione: una scheda di acquisizione, nessun dispositivo di input](view-only.svg)

Il banco più semplice. Puoi guardare dal vivo, registrare, fare screenshot a piena risoluzione e leggere il testo a schermo (OCR). Anche gli agenti possono guardare, ma nessuno può premere pulsanti.

**Componenti**

- Console nella sua base
- Scheda di acquisizione HDMI USB (UVC)
- Cavo HDMI e cavo USB della scheda

**Passaggi**

1. Collega l'uscita HDMI della base alla scheda di acquisizione.
2. Collega la scheda di acquisizione al computer.
3. Avvia NS2Bench e apri `http://127.0.0.1:7630` (o l'app desktop).
4. Scegli la tua scheda nel pannello **Video**.
5. In **Impostazioni → Dispositivi**, imposta **Connessione di input** su **Sola visualizzazione** e fai clic su **Applica connessione di input**.

**Verifica che funzioni**

- L'indicatore **video** nella barra in alto diventa verde e compare l'immagine della console.
- L'indicatore **Bridge** dice *not configured*. Qui è normale.

**Problemi comuni**

- *Immagine nera:* riattiva la console; assicurati che sia nella base e che la base sia alimentata. Prova un altro cavo HDMI.
- *Scheda assente dall'elenco:* su macOS, consenti l'accesso alla Fotocamera. Su Linux o Windows, chiudi qualsiasi app che stia usando la scheda.
- *Niente audio:* su macOS, consenti l'accesso al Microfono. Controlla che la scheda sia la sorgente audio selezionata.

## Scheda Linux USB gadget

![Scheda Linux: USB alla console, Ethernet al computer](input-linux-board.svg)

Una piccola scheda Linux si finge un insieme di dispositivi USB. Esegue **NS2Bench Bridge** (`ns2bench-board`). È la configurazione testata (Luckfox Pico Pro/Max). Supporta entrambe le modalità controller:

- **Standard** (predefinita): la console vede un controller cablato stile HORI + tastiera USB + mouse USB.
- **Pro**: la console vede un Controller Pro per Switch 2, con tutti i pulsanti compresi C, GL e GR. In questa modalità niente tastiera né mouse.

Cambia modalità con **La console vede** nel pannello di guida. Ci vogliono circa 8 secondi.

**Componenti**

- Una scheda Linux con modalità dispositivo USB (OTG): Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, o Pi 4/5 tramite la porta USB-C
- Un cavo USB dalla porta **OTG** della scheda alla porta USB della base
- Un cavo Ethernet verso il computer (oppure usa il Wi-Fi o la rete USB della scheda)
- La configurazione della scheda di acquisizione vista sopra

**Passaggi**

1. Installa NS2Bench Bridge sulla scheda dall'app: **Impostazioni → Installa → Scheda Bridge**, inserisci il suo host SSH e fai clic su **Installa sulla scheda**. (Dal sorgente: `make board-install BOARD=<ssh-host>`; aggiungi `BOARD_TARGET=aarch64-unknown-linux-musl` per un Raspberry Pi a 64 bit.)
2. Collega la porta USB **OTG** della scheda alla porta USB della base.
3. Collega l'Ethernet della scheda al computer (oppure collega entrambi alla stessa rete privata).
4. Avvia il Bridge se non è già in esecuzione: `ssh <ssh-host> /opt/ns2bench/ns2bench-hidd.sh start --bind <board-ip>`.
5. Assicurati che il file token `~/.ns2bench/hidd.token` sul computer corrisponda al token della scheda.
6. In **Impostazioni → Dispositivi** imposta:
   - **Connessione di input:** Bridge di rete
   - **Indirizzo del Bridge:** l'IP della scheda (predefinito `192.168.79.2`)
   - **Porta del Bridge:** `7631`
   - **File token del Bridge:** `~/.ns2bench/hidd.token`
7. Fai clic su **Applica connessione di input**.

**Verifica che funzioni**

- L'indicatore **Bridge** diventa verde e mostra il tempo di andata e ritorno e *usb configured*.
- Premi un pulsante sul Joy-Con a schermo. La console reagisce e il pulsante si illumina.

**Quando la console va in standby**

Una scheda alimentata dalla USB della console perde l'alimentazione quando la console va in standby, e si riavvia quando si risveglia. Lascia NS2Bench in esecuzione: se ne accorge entro circa tre secondi e si riconnette da solo. L'input torna a funzionare quando la scheda ha finito l'avvio e la console ha configurato la sua USB.

Perché funzioni, la scheda deve rimettersi in funzione da sola all'avvio. Aggiungi uno script di avvio per la tua scheda che esegua `/opt/ns2bench/gadget.sh` e poi `/opt/ns2bench/ns2bench-hidd.sh start`. Prova un vero ciclo di standby e risveglio con la tua scheda e la tua base.

**Problemi comuni**

- *Bridge offline · reconnecting:* controlla il cavo e l'IP della scheda. Fai un ping alla scheda. Assicurati che il Bridge sia in esecuzione su di essa.
- *Bridge connesso ma usb non configurata:* riattiva la console. Controlla che il cavo vada alla porta **OTG** della scheda, non a una porta di sola alimentazione. Alcuni cavi trasportano solo corrente.
- *Il mouse non fa nulla nel menu HOME:* è normale. La console legge il mouse, ma solo alcuni software mostrano un cursore.
- *Tastiera ignorata:* il supporto della console per le tastiere USB si trova in Impostazioni di sistema → Controller e accessori.
- Sulla scheda, ferma e avvia le cose solo con `ns2bench-hidd.sh` e `gadget.sh`. Non terminare processi per pattern.

## Raspberry Pi Pico

![Raspberry Pi Pico: USB alla console, UART verso un adattatore USB sul computer](input-pico.svg)

Un Raspberry Pi Pico (RP2040) con il firmware NS2Bench Bridge. La sua porta USB va alla console e presenta lo stesso controller + tastiera + mouse di una scheda Linux in modalità standard. Il computer comunica con esso tramite un collegamento seriale (UART) attraverso un adattatore USB-UART separato.

> Il firmware del Pico è compilato ma non è ancora stato provato su hardware reale. Le segnalazioni sono benvenute.

**Componenti**

- Raspberry Pi Pico (RP2040) e un cavo micro-USB
- Un adattatore USB-UART a **3,3 V** (CP2102, FT232 o CH340)
- Tre cavetti jumper
- La configurazione della scheda di acquisizione vista sopra

**Passaggi**

1. Flasha il Pico dall'app: **Impostazioni → Installa → Flasha Pico**, poi tieni premuto il pulsante **BOOTSEL** mentre colleghi il Pico al **computer**. L'app copia il firmware sull'unità **RPI-RP2** e il Pico si riavvia. Scollegalo. (Dal sorgente: `make pico` compila `firmware/pico/ns2bench-pico.uf2`; copialo tu sull'unità RPI-RP2.)
2. Collega il Pico all'adattatore:
   - Pico **GP0** (pin 1, UART0 TX) → adattatore **RX**
   - Pico **GP1** (pin 2, UART0 RX) ← adattatore **TX**
   - Pico **GND** (pin 3) — adattatore **GND**
   - Lascia scollegato il pin 3,3 V / 5 V dell'adattatore. Il Pico è alimentato dalla console.
3. Collega la micro-USB del Pico alla porta USB della base.
4. Collega l'adattatore al computer.
5. In **Impostazioni → Dispositivi** imposta:
   - **Connessione di input:** Bridge seriale
   - **Porta seriale:** l'adattatore (per esempio `/dev/cu.usbserial-XXXX` su macOS, `/dev/ttyUSB0` su Linux, `COM3` su Windows)
   - **Baud rate:** `921600`
6. Fai clic su **Applica connessione di input**.

Opzionale: compila con `NS2BENCH_TOKEN=... make pico` per fare in modo che il Pico richieda un token (imposta lo stesso file token sul computer). Compila con `NS2BENCH_BAUD=115200` se il tuo adattatore non supporta 921600.

**Verifica che funzioni**

- L'indicatore **Bridge** diventa verde e mostra *usb configured*.
- La pressione di un pulsante sul Joy-Con a schermo arriva alla console.

**Problemi comuni**

- *Non compare nessuna porta seriale:* installa il driver dell'adattatore (CP210x, FTDI o CH340), poi ricollegalo.
- *Bridge offline:* probabilmente TX e RX sono invertiti. TX va sempre su RX. Controlla anche GND.
- *Risposte illeggibili o assenti:* il baud rate deve corrispondere al firmware (921600, a meno che tu non l'abbia compilato con un altro valore).
- *Non usare mai un adattatore a 5 V.* I pin del Pico sono solo a 3,3 V.

## Modulo CH9329

![CH9329: USB alla console, UART al computer](input-ch9329.svg)

Un modulo CH9329 trasforma comandi seriali in una tastiera e un mouse USB. È economico e semplice, ma **non ha un controller**: niente pulsanti, niente levette. Usalo solo per scrivere e puntare.

**Componenti**

- Un modulo CH9329
- Un adattatore USB-UART a 3,3 V, a meno che il cavo del modulo non abbia l'USB-seriale integrata
- La configurazione della scheda di acquisizione vista sopra

**Passaggi**

1. Collega il lato USB (HID) del modulo alla porta USB della base.
2. Collega il lato UART all'adattatore: TX del modulo → RX dell'adattatore, RX del modulo ← TX dell'adattatore, GND — GND. (Salta questo passaggio se il modulo ha l'USB-seriale integrata.)
3. Collega l'adattatore (o il cavo seriale del modulo) al computer.
4. In **Impostazioni → Dispositivi** imposta:
   - **Connessione di input:** CH9329
   - **Porta seriale:** la porta dell'adattatore
   - **Baud rate:** l'impostazione del modulo, di solito `9600`
5. Fai clic su **Applica connessione di input**.

**Verifica che funzioni**

- L'indicatore **Bridge** diventa verde.
- Ciò che scrivi nella modalità tastiera nativa compare sulla console (per esempio in un campo di testo).

**Problemi comuni**

- *I pulsanti del controller non fanno nulla:* è previsto. CH9329 è solo tastiera e mouse.
- *Non succede nulla:* controlla il baud rate e inverti TX/RX se necessario.
- *Tastiera ignorata dalla console:* controlla Impostazioni di sistema → Controller e accessori → Tastiera USB.

## Opzionale: rete offline per la console

![Tre modi per dare alla console una rete offline](network.svg)

Ti serve solo per strumenti di rete locale, come i loader homebrew. Acquisizione e input non ne hanno mai bisogno. Le varianti A–C usano una rete separata per la console. La variante D la lascia sulla tua Wi-Fi di casa.

Qualunque variante tu scelga, la console **non ha internet**. NS2Bench non inoltra mai il traffico, non attiva mai la condivisione internet, NAT o un proxy, e non interroga mai un server DNS esterno. Il suo DNS locale risponde solo al test di connessione della console; ogni altro nome viene rifiutato. Ogni ricerca che la console tenta compare nel log **network**.

Il collegamento del Bridge dal computer a una scheda Linux è separato. Può usare la tua normale LAN.

### A · Cavo diretto (macOS o Linux)

**Componenti:** un adattatore Ethernet in più sul computer dedicato solo alla console, un cavo Ethernet e la porta LAN della base (oppure un adattatore USB-Ethernet sulla console).

1. Assegna all'adattatore dedicato un indirizzo privato fisso, per esempio `192.168.80.1/24`. NS2Bench non configura l'interfaccia per te.
2. Descrivi quell'interfaccia in `bench.json` (usa il nome dell'interfaccia del tuo adattatore):
   ```json
   "network": { "interface": "en7", "address": "192.168.80.1", "subnet": "192.168.80.0/24",
                "pool_start": "192.168.80.10", "pool_end": "192.168.80.13" }
   ```
   Compila il servizio di rete: `make netd`.
3. Verifica la configurazione senza aprire alcuna porta: `native/target/release/ns2bench-netd --check bench.json`.
4. Installalo come servizio:
   - **macOS:** copia il binario in `~/.ns2bench/bin/ns2bench-netd`, compila `launchd/ns2bench-netd.plist.template` e copialo in `~/Library/LaunchAgents/`. Non servono permessi di root.
   - **Linux:** installalo in `/opt/ns2bench/ns2bench-netd` e usa l'unità systemd `launchd/ns2bench-netd.service`.
5. Collega l'Ethernet della console all'adattatore dedicato.
6. In **Impostazioni → Rete**, scegli **Diretto e isolato: computer ↔ console**. Attiva DHCP e DNS locali e la pagina del test di connessione secondo necessità, poi applica.

La console riceve un indirizzo dal pool (per esempio `192.168.80.10`–`192.168.80.13`). La scheda **Rete** mostra lo stato e i lease in tempo reale.

Il servizio si rifiuta di partire se l'IP forwarding è attivo, o se non riesce a verificarlo. È voluto.

### B · Router isolato, VLAN o Wi-Fi (qualsiasi OS)

1. Configura un router, una VLAN o una rete Wi-Fi per la console con **internet bloccato, IPv6 compreso**. Una normale LAN domestica non è isolata.
2. Lascia che sia il router ad assegnare gli indirizzi (DHCP).
3. Collega la console tramite Ethernet o Wi-Fi.
4. In **Impostazioni → Rete**, scegli **Router / VLAN / Wi-Fi isolato**. Questo disattiva DHCP e DNS locali di NS2Bench, così non vanno in conflitto con il router.

### C · Computer Windows con un helper remoto

Windows non può eseguire il servizio di rete in locale. Eseguilo invece su una scheda o un PC Linux e controllalo da Windows.

1. Sull'helper Linux, configura `ns2bench-netd` come nella variante A. L'helper gestisce l'Ethernet della console.
2. Crea un token: `openssl rand -hex 16 > ~/.ns2bench/netd-control.token`. Copia lo stesso file sul computer Windows.
3. Nella configurazione dell'helper, aggiungi un listener di gestione su un indirizzo privato che **non** sia sulla rete della console:
   ```json
   "network": { "control": { "listen": "192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
4. Nella configurazione di NS2Bench su Windows, punta all'helper:
   ```json
   "network": { "helper": { "url": "http://192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
5. Mantieni privato il collegamento di gestione: un cavo diretto o la rete USB della scheda. Il suo traffico è HTTP in chiaro.

La scheda **Rete** su Windows mostra quindi lo stato e i controlli dell'helper. Oppure salta l'helper e usa la variante B.

### D · Rete di casa condivisa (Wi-Fi, nessun hardware aggiuntivo)

![Console sulla Wi-Fi di casa senza internet: il loro gateway è il loro stesso indirizzo e il computer risponde solo al loro test di connessione](shared-network.svg)

Le console restano sulla tua normale Wi-Fi di casa, accanto al computer. Niente altro da comprare o collegare. Ogni console usa **impostazioni manuali** che non le lasciano nessun posto dove mandare traffico, e NS2Bench sul tuo computer risponde solo al suo test di connessione, così la console dice comunque di essere connessa.

**Come viene bloccato internet**

| Via d'uscita | Cosa la blocca |
|---|---|
| Cercare un nome Nintendo (eShop, aggiornamenti, account, telemetria) | Il DNS di NS2Bench rifiuta ogni nome tranne il test di connessione. Ogni tentativo compare nel controllo perdite e nel log **network**. |
| Connettersi a un server tramite indirizzo IP | Il gateway della console è **il suo stesso indirizzo**, quindi niente di ciò che invia esce dalla tua rete di casa, qualunque sia l'indirizzo. |
| IPv6 | Il router non deve distribuire IPv6, altrimenti disattivalo. |
| Il computer inoltra il traffico | La console non manda mai traffico al computer come gateway, e l'helper si ferma se l'inoltro o la condivisione internet vengono attivati. |
| La console torna alle impostazioni automatiche (un ripristino, un aggiornamento, qualcuno che le cambia) | Il **controllo perdite** ti avvisa entro circa 10 minuti. Aggiungi anche una pausa dal router (passo 7). |

L'ultima riga è l'unica cosa che una rete condivisa non può escludere: lì il lucchetto sono le impostazioni della console stessa. Se ti serve "offline anche dopo un ripristino di fabbrica", usa la variante A o B, dove non esiste proprio una strada verso internet.

**Ti serve:** NS2Bench su macOS o Linux con l'helper di rete installato, il computer e le console sulla stessa rete di casa, e un router che non distribuisce IPv6 (o con IPv6 disattivato).

1. **Riserva gli indirizzi nel router** (Google Home, l'app del router o la sua pagina web): uno per il computer e uno per ogni console. Annotali, per esempio computer `192.168.86.199`, console `192.168.86.251` e `192.168.86.252`.
2. **Installa l'helper di rete** se non l'hai già fatto: **Impostazioni → Installa → Helper di rete**.
3. Apri **Impostazioni → Rete** e scegli **Rete di casa condivisa**.
4. **Scegli la scheda di rete** che si trova sulla rete delle console. L'elenco mostra ogni scheda con il suo indirizzo, per esempio `en10 · Belkin USB-C LAN · 192.168.86.199/24`.
5. Inserisci gli **indirizzi delle console**, spunta le tre caselle (niente IPv6, indirizzo del computer riservato, nessun'altra Wi-Fi salvata sulle console) e fai clic su **Salva e usa questa rete**. NS2Bench conserva un backup del tuo file di impostazioni. Poi fai clic su **Riavvia helper di rete**.
6. **Su ogni console:** Impostazioni di sistema → Internet → la tua Wi-Fi → **Modifica impostazioni**:
   - Impostazioni indirizzo IP: **Manuale**. Indirizzo IP: l'indirizzo riservato della console. Subnet mask: `255.255.255.0`. **Gateway: l'indirizzo della console stessa.**
   - Impostazioni DNS: **Manuale**. DNS primario e secondario: **l'indirizzo del tuo computer**.
   - Salva ed esegui il test di connessione. Passa, e la console dice di essere connessa.
   NS2Bench mostra questi valori esatti per ogni console sotto **Inserisci questo su ogni console**.
7. **Secondo lucchetto consigliato:** nell'app del router, metti in pausa o blocca internet per le console (Google Home: Wi-Fi → Family Wi-Fi → pausa). Controlla che il test di connessione passi ancora con la pausa attiva.
8. Elimina tutte le altre reti Wi-Fi salvate su ogni console ed evita il gioco wireless locale con una console con firmware più recente (le console possono passarsi aggiornamenti di sistema in quel modo).

**Leggere il controllo perdite** (Impostazioni → Rete, controllato ogni 30 secondi):

- **Protetta:** la console ha interrogato NS2Bench di recente. Mostra quando, il suo ultimo test di connessione e quanti nomi sono stati rifiutati.
- **Sulla tua rete ma non usa NS2Bench:** non chiede nulla da 10 minuti pur essendo sulla rete. Potrebbe essere tornata alle impostazioni automatiche ed essere online. Controlla le sue impostazioni. NS2Bench invia anche una notifica sul desktop.
- **Non è sulla rete al momento:** spenta, in standby o lontana. Niente da fare.

**Problemi comuni**

- *La console non si connette:* controlla che il suo DNS sia l'indirizzo del computer, che l'helper sia in esecuzione (Impostazioni → Installa) e che la scheda scelta al passo 4 sia sulla rete delle console. Ricevono risposta solo gli indirizzi in elenco, quindi controlla anche l'IP della console.
- *La console rifiuta il proprio indirizzo come gateway:* usa invece un indirizzo libero fuori dall'intervallo DHCP del router, per esempio `192.168.86.254`. Assicurati che niente lo usi, né ora né in futuro.
- *L'indirizzo del computer è cambiato:* le console non riescono a connettersi (restano offline). Riservalo nel router e ripeti la configurazione.
- *Una rete alla volta:* l'helper serve o questa rete condivisa o un cavo diretto (variante A), non entrambi.

**Problemi comuni**

- *La scheda Rete dice non disponibile:* il servizio non è in esecuzione, oppure il servizio e l'interfaccia usano cartelle di stato diverse. Su Windows, controlla `network.helper` e che entrambi i file token corrispondano.
- *La console non riceve un indirizzo:* controlla il nome dell'interfaccia e la subnet nella configurazione, e che il cavo vada all'adattatore dedicato.
- *La console dice di non avere internet:* è corretto. Il test di connessione passa in locale, ma niente va oltre.

## Agenti

Quando gli indicatori sono verdi, gli agenti possono usare il banco. Apri **Impostazioni → API e MCP** per l'indirizzo API, esempi di richieste e una configurazione MCP pronta da copiare per Claude Code, Codex o opencode. Agenti e persone condividono gli stessi controlli, e l'interruttore solo-manuale blocca l'input degli agenti in qualsiasi momento.

## Note per Windows

- L'acquisizione usa `ffmpeg` con DirectShow. Chiudi le altre app che tengono occupata la scheda.
- Scheda Linux, Pico e CH9329 funzionano tutti da Windows. Le porte seriali si chiamano `COM3`, `COM4` e così via.
- Su Windows non c'è un servizio di rete locale. Usa un helper remoto (variante C) o un router isolato (variante B).
