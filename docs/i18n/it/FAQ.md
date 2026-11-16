# Generale

## Che cos’è NS2Bench?

Un banco di prova per una Nintendo Switch 2 sulla tua scrivania. Mostra l’immagine e l’audio della console sul computer e permette a te, o a un agente IA, di premerne i pulsanti. Puoi registrare clip, leggere il testo sullo schermo ed eseguire test ripetibili.

## A chi è rivolto?

A sviluppatori che testano software su hardware reale, a chi automatizza controlli ripetitivi, a esperimenti di accessibilità e a chiunque voglia che un agente IA usi una console mentre guarda.

## Modifica la mia console?

No. NS2Bench fa solo ciò che fanno una TV e un controller con cavo: riceve l’HDMI e preme pulsanti via USB. Sulla console non viene installato nulla. Funziona con qualsiasi Switch 2 così com’è appena tolta dalla scatola, con qualsiasi versione di sistema: non servono jailbreak, modifiche né homebrew. Funziona anche una console con jailbreak.

## La console va online?

No. NS2Bench non dà mai accesso a internet alla console. Acquisizione e input USB non richiedono alcuna rete. La rete opzionale per la console risponde solo al suo test di connessione e non inoltra mai traffico. Vedi [Rete offline per la console](#offline-console-network).

## È gratuito?

Sì. NS2Bench è gratuito e open source con licenza GPL-3.0. Puoi usarlo, modificarlo e condividerlo. Quando lo distribuisci, modificato o no, mantieni la dicitura "NS2Bench, created by PhantomPtr" e la licenza; se distribuisci una versione modificata, offri il codice sorgente a chi la riceve e indica che è modificata (vedi NOTICE). Se ti è utile, puoi [sostenerlo](README.md#support).

## È affiliato a Nintendo?

No. NS2Bench è un progetto indipendente. Nintendo Switch è un marchio di Nintendo.

# Configurazione

## Cosa mi serve?

Una scheda di acquisizione per video e audio (sempre) e un dispositivo di input se vuoi premere pulsanti:

- una scheda Linux con porta USB device (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5),
- oppure un Raspberry Pi Pico con un adattatore USB-UART a 3,3 V,
- oppure un modulo CH9329 (solo tastiera e mouse).

La [guida alla configurazione](docs/setup/README.md) ha uno schema e i passaggi per ciascuno.

## Cosa devo installare sul computer?

- **macOS:** niente; consenti solo Fotocamera, Microfono e Rete locale. `brew install ffmpeg` se vuoi salvare clip.
- **Windows:** ffmpeg (`winget install Gyan.FFmpeg`) e un driver CH340/CP210x se l’adattatore seriale lo richiede.
- **Linux:** ffmpeg (lo installa il `.deb`) e i gruppi `video` e `dialout` (`sudo usermod -aG video,dialout $USER`).

Vedi [Prima di iniziare](docs/setup/README.md#before-you-start-your-computer).

## Quale dispositivo di input scelgo?

Se vuoi tutti i pulsanti, compresi C, GL e GR, usa una scheda Linux: la sua modalità Pro presenta alla console un vero Controller Pro di Switch 2. Un Pico è l’opzione controller più economica (tutti i pulsanti tranne C, GL e GR). Un CH9329 offre solo tastiera e mouse. Puoi iniziare con la sola visualizzazione e aggiungere un dispositivo di input più avanti.

## Mi serve la base?

La console deve poter uscire in HDMI, il che di solito significa la base. Il dispositivo di input si collega a una delle porte USB della base.

## Posso usarlo senza dispositivo di input?

Sì. Scegli **Sola visualizzazione (nessun dispositivo di input)** in Impostazioni → Dispositivi. Puoi guardare, ascoltare, fare screenshot, salvare clip e leggere il testo sullo schermo.

## Dove imposto la scheda di acquisizione?

Nel pannello **Video** a destra della finestra principale. Elenca solo i formati offerti dalla tua scheda.

## Dove imposto il dispositivo di input?

In **Impostazioni → Dispositivi**. Le modifiche si applicano senza riavviare; la vecchia connessione rilascia prima tutti i pulsanti.

## L’app può installare il Bridge, il firmware del Pico o l’helper di rete?

Sì, in **Impostazioni → Installa**. **Flasha Pico** mette il firmware Bridge su un Raspberry Pi Pico (tieni premuto BOOTSEL mentre lo colleghi). **Installa sulla scheda** installa o aggiorna il Bridge su una scheda Linux via SSH (configura prima l’accesso con chiave). **Installa helper** esegue l’helper di rete offline come servizio su macOS o Linux. I download provengono dalla release corrispondente all’app e vengono verificati prima dell’uso.

## Come capisco che funziona?

L’indicatore **video** nella barra in alto diventa verde con il frame rate, e l’indicatore **Bridge** diventa verde quando la console ha accettato il dispositivo di input. **Diagnostica** nella barra in alto copia un riepilogo completo dello stato.

# Acquisizione e audio

## Quali schede di acquisizione funzionano?

Qualsiasi scheda di acquisizione HDMI USB (UVC). L’obiettivo è 1080p60. Testata: UGREEN 15389 (MS2130), a 1080p60 e 1440p30.

Su Windows funzionano anche le schede interne (PCIe) come la Elgato 4K Pro, se in OBS compaiono come **Video Capture Device**. Scegli la scheda in **Video → Dispositivo** e il suo audio in **Video → Audio** (oppure lascia Audio su automatico). NS2Bench chiede alla scheda una dimensione e una frequenza dei fotogrammi che offre. Se la scheda rifiuta comunque, NS2Bench ripiega sull’impostazione predefinita della scheda, come fa OBS. Se i colori sembrano slavati, disattiva l’HDR sulla console oppure l’uscita HDR nel software della scheda.

## Posso usare OBS contemporaneamente?

Su macOS sì: NS2Bench condivide la scheda con OBS e non gliela toglie mai. Su Windows e Linux, ffmpeg di solito ha bisogno della scheda in esclusiva, quindi chiudi le altre app che la usano.

## Non c’è immagine.

Controlla che la console sia nella base e accesa, che il cavo HDMI sia nell’**ingresso** della scheda e che nel pannello Video sia selezionata la scheda giusta. Su macOS, consenti la **Fotocamera** a NS2Bench in Impostazioni di Sistema → Privacy e sicurezza. Esegui **Diagnostica** per i dettagli.

## Non c’è audio.

Fai clic su **Audio disattivato** nella barra in alto per attivare l’audio. Su macOS, consenti il **Microfono** a NS2Bench. Se hai due schede di acquisizione identiche, NS2Bench abbina l’audio all’immagine in base alla posizione USB.

## Quanta CPU usa l’acquisizione?

Su macOS circa il 12% di un core a 1080p60 (JPEG e H.264 hardware). Su Linux il flusso delle schede MJPEG passa senza ricodifica.

# Dispositivi di input

## L’indicatore Bridge resta rosso.

- **Scheda Linux:** controlla che la scheda sia alimentata e raggiungibile all’indirizzo in Impostazioni → Dispositivi, e che il file del token corrisponda al token della scheda.
- **Pico:** controlla la porta seriale e che il baud rate sia 921600; TX e RX devono essere incrociati (TX dell’adattatore → GP1 del Pico, RX dell’adattatore → GP0 del Pico) e GND deve essere collegato.
- **CH9329:** usa il baud rate del modulo (di solito 9600).

Poi controlla che il dispositivo di input sia collegato alla porta USB della console e che la console sia attiva.

## Le pressioni non vengono registrate.

Switch 2 richiede che una pressione duri almeno circa 50 ms. NS2Bench tiene ogni pressione per almeno 75 ms. Se Bridge è verde ma non succede nulla, la console potrebbe essere in standby o con lo schermo oscurato: la prima pressione la riattiva soltanto.

## Come ottengo C, GL e GR?

GL e GR sono i due pulsanti posteriori del Controller Pro di Nintendo Switch 2; i Joy-Con 2 non li hanno. Per usarli (insieme a C), usa una scheda Linux e scegli **La console vede → Controller Pro** nel pannello Comando attivo. La console vedrà allora un Controller Pro di Switch 2. Altrimenti C funziona come scorciatoia di GameChat.

## Posso usare il mio gamepad?

Sì. Collegalo al computer; NS2Bench lo mappa in base alla posizione dei pulsanti (il pulsante frontale in basso è B).

## Posso scrivere con una tastiera?

Scegli **La tua tastiera invia → Tastiera e mouse** nel pannello Comando attivo. La console vede una vera tastiera USB. Serve una scheda Linux, un Pico in modalità standard o un CH9329.

## Cosa succede quando la console va in standby?

Una scheda alimentata dalla USB della console perde l’alimentazione e si riavvia quando la console si riattiva. NS2Bench si riconnette da solo. Le pressioni fatte mentre Bridge è offline vengono scartate, mai ripetute dopo.

## NS2Bench può risvegliare la console dallo standby?

Sì, con un piccolo aiuto. Uno schermo oscurato si riattiva con qualsiasi pressione, e NS2Bench lo fa da solo. La modalità standby, un crash o la console spenta richiedono una di queste soluzioni, da impostare in **Impostazioni → Dispositivi → Alimentazione e riattivazione**:

- **HDMI-CEC (niente di meccanico):** un adattatore USB-CEC (per esempio di Pulse-Eight) sul cavo HDMI tra la base e la scheda di acquisizione invia l'"accendi" che invierebbe una TV. Attiva sulla console **Impostazioni della console → Uscita TV → Sincronizza con lo stato di accensione della TV** e installa libcec sul computer (`brew install libcec` su Mac). Può anche mettere la console in standby.
- **Un comando del pulsante di accensione:** per un premi-pulsante come SwitchBot, un webhook di Home Assistant o un relè. È l'unico modo per tornare da un crash o dalla console spenta.

Poi **Riattiva la console** (nella schermata senza immagine, nella ricerca, nell'API `POST /api/power`, nello strumento MCP `power` e con `ns2bench power wake`) prova CEC, poi il pulsante di accensione se lo consenti, poi una pressione. Switch 2 non ha il Wake-on-LAN, e una scheda alimentata dalla porta USB della console si spegne circa 7 secondi dopo la modalità standby; si riavvia quando la console si risveglia, e NS2Bench si ricollega da solo in circa 30 secondi.

# Rete offline per la console

## Mi serve una rete?

No. Acquisizione e input USB funzionano senza rete. La rete della console serve solo agli strumenti che comunicano con la console via LAN.

## Come do alla console una LAN senza internet?

Collega l’Ethernet della console (porta della base o adattatore USB) a un adattatore Ethernet dedicato sul tuo Mac o computer Linux ed esegui `ns2bench-netd`. Assegna gli indirizzi e risponde solo al test di connessione. Oppure usa un router, una VLAN o una rete Wi-Fi con internet bloccato. Vedi la [guida alla configurazione](docs/setup/README.md#optional-offline-console-network).

## Posso disattivare il blocco di internet?

No. Non è un’impostazione. NS2Bench non inoltra, non fa NAT né proxy del traffico della console e non interroga mai un server DNS esterno.

## Uso Windows.

Il servizio di rete funziona su macOS e Linux. Su Windows, eseguilo su una scheda Linux o un PC che gestisce la porta Ethernet della console e controllalo dalla scheda Rete ([helper remoto](docs/REFERENCE.md#remote-helper-windows-hosts)), oppure usa un router con internet bloccato.

# Agenti e API

## Quali agenti funzionano?

Qualsiasi client MCP: Claude Code, Codex, opencode e altri. Impostazioni → API e MCP mostra una configurazione pronta da copiare.

## Posso fermare un agente?

Sì. **Blocca agenti (solo manuale)** nel pannello Comando attivo ferma subito l’input degli agenti. Solo tu, dalla finestra di NS2Bench, puoi disattivarlo di nuovo.

## L’API è sicura sulla mia rete?

Il motore resta in ascolto solo su `127.0.0.1` per impostazione predefinita. Per ascoltare su un altro indirizzo serve un token. Le pagine web di altri siti non possono inviargli comandi.

## Dov’è documentata l’API?

In [docs/REFERENCE.md](docs/REFERENCE.md#agents-mcp-and-http-api).

# App e aggiornamenti

## Come funzionano gli aggiornamenti?

L’app controlla ogni giorno le release su GitHub e avvisa quando c’è un aggiornamento. Ogni download viene verificato con lo SHA-256 della release prima dell’installazione. Download e installazione automatici sono disattivati per impostazione predefinita; attivali in Impostazioni → Aggiornamenti.

## macOS dice che l’app è danneggiata o non può essere verificata.

L’app non è ancora autenticata (notarizzata). Esegui una volta `xattr -dr com.apple.quarantine /Applications/NS2Bench.app`, poi aprila.

## Windows dice di aver protetto il PC.

Scegli **Ulteriori informazioni → Esegui comunque**. Il programma di installazione non è ancora firmato con un certificato a pagamento.

## Dopo un aggiornamento macOS chiede di nuovo Fotocamera o Microfono.

Dalla 0.0.3 ogni versione di NS2Bench ha la stessa firma, quindi macOS chiede una volta e se lo ricorda, anche dopo riavvii e aggiornamenti. Aggiornando dalla 0.0.2 o precedenti, chiede un’ultima volta.

## Come cambio tema o lingua?

Impostazioni → Aspetto. Il pulsante con la luna nella barra in alto passa da chiaro a scuro.

## Dove sono clip, screenshot e log?

In `~/.ns2bench/`: clip, report dei test e un file di log al giorno in `~/.ns2bench/logs` (conservati 7 giorni). Gli screenshot finiscono nella cartella Download. Non viene caricato nulla da nessuna parte.

## NS2Bench può avvisarmi?

Sì. **Impostazioni → Notifiche** attiva le notifiche desktop per quando il Bridge o l’immagine si interrompono, test e clip finiscono, un agente inizia a pilotare o un aggiornamento è pronto. Per impostazione predefinita compaiono solo mentre NS2Bench è in background.

## Come installo lo strumento da riga di comando?

**Impostazioni → Installa → Installa lo strumento da riga di comando** mette `ns2bench` nel tuo PATH (macOS potrebbe chiedere la password una volta; il `.deb` per Linux lo installa già). Poi esegui `ns2bench --help`. Può fare tutto ciò che fanno l’API e MCP.

# Risoluzione dei problemi

## Come segnalo un bug?

Fai clic su **Aiuto** nella barra in alto, poi su **Salva segnalazione** (uno zip con log, stato e uno screenshot; nessun token), e allegalo a una [issue su GitHub](https://github.com/phantomptr/ns2bench/issues/new/choose).

## Cosa significano i colori del log?

Le righe blu sono attività normale, quelle ambra sono avvisi che possono risolversi da soli e quelle rosse richiedono il tuo intervento. Per i problemi che NS2Bench riconosce, un riquadro blu sotto la riga spiega cosa è successo e cosa fare. **Problemi** nel pannello Log mostra solo avvisi ed errori; **Tutto** aggiunge dettagli come ogni chiamata API.

## Cosa fa doctor?

**Aiuto → Esegui il controllo** nell’app (o `ns2bench doctor`) controlla video, immagine della console, OCR, Bridge, stato USB, calibrazione e isolamento di rete, e ti dice quale parte non funziona e cosa fare. `--probe` preme anche RIGHT e LEFT nel menu HOME per testare l’input da capo a fondo.

## Dove posso chiedere aiuto?

Su [Discord](https://discord.gg/BqjZE6vZE).
