# Installer NS2Bench

NS2Bench vous permet, à vous ou à un agent IA, de voir, d’entendre et de piloter une Nintendo Switch 2 depuis un ordinateur.
Ce guide explique comment tout brancher.

![Vue d’ensemble de l’installation de NS2Bench](overview.svg)

Chaque installation comporte deux parties indépendantes :

- **Les yeux (toujours) :** la sortie HDMI de la console va dans une carte de capture USB. La carte transmet la vidéo et le son à l’ordinateur.
- **Les mains (facultatif) :** un petit appareil d’entrée se branche sur le port USB de la console. L’ordinateur lui envoie des commandes. La console voit une manette filaire, un clavier et une souris.

Les personnes utilisent la page NS2Bench dans un navigateur ou l’application de bureau. Les agents utilisent l’API HTTP ou MCP sur le même ordinateur (`127.0.0.1:7630`). Tout le monde partage le même banc, et vous voyez chaque action des agents en direct.

**La console reste hors ligne.** NS2Bench ne lui donne jamais accès à Internet. La capture et l’entrée USB n’ont besoin d’aucun réseau.

## De quelle installation ai-je besoin ?

| Ce que vous avez | Installation | Connexion d’entrée dans les Réglages |
|---|---|---|
| Seulement une carte de capture | [Visualisation seule](#view-only) | Visualisation seule |
| Une carte Linux avec mode périphérique USB (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5) | [Carte Linux](#linux-usb-gadget-board) | Bridge réseau |
| Un Raspberry Pi Pico (RP2040) et un adaptateur USB-UART 3,3 V | [Pico](#raspberry-pi-pico) | Bridge série |
| Un module CH9329 (clavier et souris uniquement) | [CH9329](#ch9329-module) | CH9329 |
| Vous voulez aussi mettre la console sur un réseau local (outils homebrew) | [Réseau hors ligne](#optional-offline-console-network) | (onglet Réseau) |

Vous hésitez ? Commencez par la **visualisation seule**, puis ajoutez un appareil d’entrée plus tard. Les réglages d’entrée se changent sans redémarrer l’application.

Si vous voulez tous les boutons de la manette, y compris C, GL et GR, utilisez une carte Linux. Son mode **Pro** présente à la console une manette Pro Switch 2. Le Pico et le mode standard de la carte couvrent tous les boutons sauf C, GL et GR. Le CH9329 n’a aucune manette.

## Avant de commencer : votre ordinateur

Installez l’application depuis la [dernière version](https://github.com/phantomptr/ns2bench/releases/latest), puis :

### macOS (14 ou plus récent)

- **Rien d’autre à installer.** La capture, le son et la lecture du texte à l’écran sont intégrés à l’application.
- Premier lancement : si macOS indique qu’Apple n’a pas pu vérifier le .dmg, ou que l’application est endommagée ou ne peut pas être vérifiée, lancez `xattr -dr com.apple.quarantine ~/Downloads/NS2Bench-*.dmg` avant d’ouvrir le .dmg (ou `xattr -dr com.apple.quarantine /Applications/NS2Bench.app` une fois installée), ou cliquez sur **Ouvrir quand même** dans Réglages Système → Confidentialité et sécurité. Seulement pour la première installation : les mises à jour depuis l’app n’en ont pas besoin.
- Autorisez **Caméra** (l’image), **Micro** (le son de la console) et **Réseau local** (l’appareil d’entrée) quand c’est demandé. Vous pouvez les modifier plus tard dans Réglages Système → Confidentialité et sécurité.
- Les **clips** (Enregistrer un clip) ont besoin de ffmpeg : `brew install ffmpeg`.
- Les adaptateurs série (Pico, CH9329) fonctionnent sans pilote ; ils apparaissent comme `/dev/cu.usbserial-…` ou `/dev/cu.usbmodem…`.

### Windows 10/11

- **Installez ffmpeg** (la capture, le son et les clips l’utilisent) : `winget install Gyan.FFmpeg`, puis redémarrez NS2Bench. NS2Bench trouve aussi ffmpeg installé avec Chocolatey ou Scoop, ou un `ffmpeg.exe` placé à côté de `ns2bench-app.exe`.
- SmartScreen peut afficher « Windows a protégé votre ordinateur » : choisissez **Informations complémentaires → Exécuter quand même**.
- Les adaptateurs USB-UART peuvent nécessiter le pilote du fabricant (CH340/CH341 ou CP210x). Le port apparaît comme `COM3` ou similaire dans le Gestionnaire de périphériques.
- Facultatif : [Tesseract](https://github.com/UB-Mannheim/tesseract/wiki) pour la lecture du texte à l’écran.
- Le service réseau hors ligne de la console ne fonctionne pas sous Windows : utilisez un [assistant Linux ou un routeur](#optional-offline-console-network).

### Linux

- **ffmpeg :** le `.deb` l’installe pour vous. Avec l’AppImage, installez-le vous-même (`sudo apt install ffmpeg`, `sudo dnf install ffmpeg`, `sudo pacman -S ffmpeg`).
- **Permissions :** ajoutez-vous aux groupes propriétaires des cartes de capture et des ports série, puis déconnectez-vous et reconnectez-vous :
  `sudo usermod -aG video,dialout $USER` (Arch : `video,uucp`).
- AppImage : `chmod +x NS2Bench-*.AppImage`. Si elle ne démarre pas, installez FUSE 2 (`sudo apt install libfuse2`).
- Facultatif : `sudo apt install tesseract-ocr` pour la lecture du texte à l’écran.
- Les ports série apparaissent comme `/dev/ttyACM0` (Pico) ou `/dev/ttyUSB0` (la plupart des adaptateurs).

## La carte de capture (toutes les installations)

- N’importe quelle carte de capture HDMI USB **UVC** convient. L’objectif est 1080p60. Testée : UGREEN 15389 (MS2130).
- Branchez la **sortie HDMI** de la station d’accueil sur l’entrée HDMI de la carte. Branchez la carte à l’ordinateur en USB.
- Choisissez la carte, la résolution, la fréquence d’images et la qualité dans le panneau **Vidéo**. Il n’affiche que ce que votre carte propose.
- Le **son** vient aussi de la carte de capture. Il n’y a pas de câble audio séparé.
- **macOS :** un petit assistant intégré s’occupe de la capture. Il partage la carte avec OBS : si OBS utilise déjà la carte, NS2Bench reprend le format d’OBS et ne lui retire jamais la carte. Au premier lancement, autorisez **Caméra** (image), **Micro** (son de la console) et **Réseau local** (le Bridge).
- **Linux et Windows :** NS2Bench utilise `ffmpeg` (installez-le d’abord). La carte doit parfois être libre : fermez OBS ou les autres applications qui l’utilisent si l’image ne démarre pas.
- Lancez `ns2bench devices` pour lister les cartes de capture que NS2Bench voit.

## Visualisation seule

![Visualisation seule : une carte de capture, sans appareil d’entrée](view-only.svg)

Le banc le plus simple. Vous pouvez regarder en direct, enregistrer, faire des captures d’écran en pleine résolution et lire le texte à l’écran (OCR). Les agents peuvent aussi regarder, mais personne ne peut appuyer sur les boutons.

**Matériel**

- La console dans sa station d’accueil
- Une carte de capture HDMI USB (UVC)
- Un câble HDMI et le câble USB de la carte

**Étapes**

1. Reliez la sortie HDMI de la station d’accueil à la carte de capture.
2. Branchez la carte de capture sur l’ordinateur.
3. Démarrez NS2Bench et ouvrez `http://127.0.0.1:7630` (ou l’application de bureau).
4. Choisissez votre carte dans le panneau **Vidéo**.
5. Dans **Réglages → Appareils**, réglez **Connexion d’entrée** sur **Visualisation seule** et cliquez sur **Appliquer la connexion d’entrée**.

**Vérifier que ça marche**

- La pastille **vidéo** de la barre du haut passe au vert et l’image de la console apparaît.
- La pastille **Bridge** indique *non configuré*. C’est normal ici.

**Problèmes courants**

- *Image noire :* réveillez la console ; vérifiez qu’elle est sur sa station d’accueil et que celle-ci est alimentée. Essayez un autre câble HDMI.
- *La carte n’apparaît pas dans la liste :* sous macOS, autorisez l’accès à la Caméra. Sous Linux ou Windows, fermez toute application qui utilise la carte.
- *Pas de son :* sous macOS, autorisez l’accès au Micro. Vérifiez que la carte est la source audio sélectionnée.

## Carte Linux USB gadget

![Carte Linux : USB vers la console, Ethernet vers l’ordinateur](input-linux-board.svg)

Une petite carte Linux se fait passer pour des périphériques USB. Elle exécute **NS2Bench Bridge** (`ns2bench-board`). C’est l’installation testée (Luckfox Pico Pro/Max). Elle prend en charge les deux modes de manette :

- **Standard** (par défaut) : la console voit une manette filaire de type HORI + un clavier USB + une souris USB.
- **Pro** : la console voit une manette Pro Switch 2, avec tous les boutons y compris C, GL et GR. Pas de clavier ni de souris dans ce mode.

Changez de mode avec **La console voit** dans le panneau de pilotage. Cela prend environ 8 secondes.

**Matériel**

- Une carte Linux avec mode périphérique USB (OTG) : Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, ou Pi 4/5 via le port USB-C
- Un câble USB entre le port **OTG** de la carte et le port USB de la station d’accueil
- Un câble Ethernet vers l’ordinateur (ou utilisez le Wi-Fi ou le réseau USB de la carte)
- L’installation de la carte de capture ci-dessus

**Étapes**

1. Installez NS2Bench Bridge sur la carte depuis l’app : **Réglages → Installer → Carte Bridge**, saisissez son hôte SSH, cliquez sur **Installer sur la carte**. (Depuis les sources : `make board-install BOARD=<ssh-host>` ; ajoutez `BOARD_TARGET=aarch64-unknown-linux-musl` pour un Raspberry Pi 64 bits.)
2. Branchez le port USB **OTG** de la carte sur le port USB de la station d’accueil.
3. Reliez l’Ethernet de la carte à l’ordinateur (ou connectez les deux au même réseau privé).
4. Démarrez le Bridge s’il ne tourne pas déjà : `ssh <ssh-host> /opt/ns2bench/ns2bench-hidd.sh start --bind <board-ip>`.
5. Vérifiez que le fichier de jeton `~/.ns2bench/hidd.token` de l’ordinateur correspond au jeton de la carte.
6. Dans **Réglages → Appareils**, réglez :
   - **Connexion d’entrée :** Bridge réseau
   - **Adresse du Bridge :** l’IP de la carte (par défaut `192.168.79.2`)
   - **Port du Bridge :** `7631`
   - **Fichier de jeton du Bridge :** `~/.ns2bench/hidd.token`
7. Cliquez sur **Appliquer la connexion d’entrée**.

**Vérifier que ça marche**

- La pastille **Bridge** passe au vert et affiche le temps aller-retour et *usb configured*.
- Appuyez sur un bouton du Joy-Con à l’écran. La console réagit et le bouton s’allume.

**Quand la console se met en veille**

Une carte alimentée par l’USB de la console perd son alimentation quand la console se met en veille, et redémarre quand elle se réveille. Laissez NS2Bench tourner : il le détecte en trois secondes environ et se reconnecte tout seul. L’entrée refonctionne dès que la carte a démarré et que la console a configuré son USB.

Pour cela, la carte doit se relancer toute seule au démarrage. Ajoutez un script de démarrage pour votre carte qui exécute `/opt/ns2bench/gadget.sh` puis `/opt/ns2bench/ns2bench-hidd.sh start`. Testez une vraie mise en veille et un vrai réveil avec votre carte et votre station d’accueil.

**Problèmes courants**

- *Bridge hors ligne · reconnexion :* vérifiez le câble et l’IP de la carte. Faites un ping vers la carte. Vérifiez que le Bridge y tourne.
- *Bridge connecté mais l’USB n’est pas configuré :* réveillez la console. Vérifiez que le câble va bien au port **OTG** de la carte, pas à un port d’alimentation seule. Certains câbles ne transportent que le courant.
- *La souris ne fait rien sur le menu HOME :* c’est normal. La console lit la souris, mais seuls certains logiciels affichent un curseur.
- *Clavier ignoré :* la prise en charge du clavier USB de la console se trouve dans Paramètres de la console → Manettes et accessoires.
- Sur la carte, arrêtez et démarrez les choses uniquement avec `ns2bench-hidd.sh` et `gadget.sh`. Ne tuez pas de processus par motif.

## Raspberry Pi Pico

![Raspberry Pi Pico : USB vers la console, UART vers un adaptateur USB sur l’ordinateur](input-pico.svg)

Un Raspberry Pi Pico (RP2040) avec le firmware NS2Bench Bridge. Son port USB va à la console et présente la même manette + clavier + souris qu’une carte Linux en mode standard. L’ordinateur lui parle par une liaison série (UART) via un adaptateur USB-UART séparé.

> Le firmware du Pico est compilé mais n’a pas encore été testé sur du vrai matériel. Les retours sont les bienvenus.

**Matériel**

- Un Raspberry Pi Pico (RP2040) et un câble micro-USB
- Un adaptateur USB-UART **3,3 V** (CP2102, FT232 ou CH340)
- Trois fils de liaison
- L’installation de la carte de capture ci-dessus

**Étapes**

1. Flashez le Pico depuis l’app : **Réglages → Installer → Flasher le Pico**, puis maintenez le bouton **BOOTSEL** enfoncé en branchant le Pico sur l’**ordinateur**. L’app copie le firmware sur le lecteur **RPI-RP2** et le Pico redémarre. Débranchez-le. (Depuis les sources : `make pico` compile `firmware/pico/ns2bench-pico.uf2` ; copiez-le vous-même sur le lecteur RPI-RP2.)
2. Câblez le Pico à l’adaptateur :
   - Pico **GP0** (broche 1, UART0 TX) → adaptateur **RX**
   - Pico **GP1** (broche 2, UART0 RX) ← adaptateur **TX**
   - Pico **GND** (broche 3) — adaptateur **GND**
   - Ne branchez pas la broche 3,3 V / 5 V de l’adaptateur. Le Pico est alimenté par la console.
3. Branchez le micro-USB du Pico sur le port USB de la station d’accueil.
4. Branchez l’adaptateur sur l’ordinateur.
5. Dans **Réglages → Appareils**, réglez :
   - **Connexion d’entrée :** Bridge série
   - **Port série :** l’adaptateur (par exemple `/dev/cu.usbserial-XXXX` sous macOS, `/dev/ttyUSB0` sous Linux, `COM3` sous Windows)
   - **Débit (bauds) :** `921600`
6. Cliquez sur **Appliquer la connexion d’entrée**.

Facultatif : compilez avec `NS2BENCH_TOKEN=... make pico` pour que le Pico exige un jeton (configurez le même fichier de jeton sur l’ordinateur). Compilez avec `NS2BENCH_BAUD=115200` si votre adaptateur ne gère pas 921600.

**Vérifier que ça marche**

- La pastille **Bridge** passe au vert et affiche *usb configured*.
- Un appui sur le Joy-Con à l’écran atteint la console.

**Problèmes courants**

- *Aucun port série n’apparaît :* installez le pilote de l’adaptateur (CP210x, FTDI ou CH340), puis rebranchez-le.
- *Bridge hors ligne :* TX et RX sont probablement inversés. TX va toujours vers RX. Vérifiez aussi GND.
- *Réponses illisibles ou absentes :* le débit doit correspondre au firmware (921600, sauf si vous l’avez compilé avec une autre valeur).
- *N’utilisez jamais un adaptateur 5 V.* Les broches du Pico supportent uniquement 3,3 V.

## Module CH9329

![CH9329 : USB vers la console, UART vers l’ordinateur](input-ch9329.svg)

Un module CH9329 transforme des commandes série en clavier et souris USB. Il est bon marché et simple, mais il **n’a pas de manette** : ni boutons, ni sticks. Utilisez-le uniquement pour taper et pointer.

**Matériel**

- Un module CH9329
- Un adaptateur USB-UART 3,3 V, sauf si le câble de votre module intègre déjà l’USB-série
- L’installation de la carte de capture ci-dessus

**Étapes**

1. Branchez le côté USB (HID) du module sur le port USB de la station d’accueil.
2. Câblez le côté UART à l’adaptateur : TX du module → RX de l’adaptateur, RX du module ← TX de l’adaptateur, GND — GND. (Sautez cette étape si le module intègre l’USB-série.)
3. Branchez l’adaptateur (ou le câble série du module) sur l’ordinateur.
4. Dans **Réglages → Appareils**, réglez :
   - **Connexion d’entrée :** CH9329
   - **Port série :** le port de l’adaptateur
   - **Débit (bauds) :** celui du module, généralement `9600`
5. Cliquez sur **Appliquer la connexion d’entrée**.

**Vérifier que ça marche**

- La pastille **Bridge** passe au vert.
- Ce que vous tapez en mode clavier natif apparaît sur la console (par exemple dans un champ de texte).

**Problèmes courants**

- *Les boutons de la manette ne font rien :* c’est normal. Le CH9329 ne gère que le clavier et la souris.
- *Rien ne se passe :* vérifiez le débit et inversez TX/RX si besoin.
- *Clavier ignoré par la console :* vérifiez Paramètres de la console → Manettes et accessoires → Clavier USB.

## Facultatif : réseau hors ligne pour la console

![Trois façons de donner à la console un réseau hors ligne](network.svg)

Vous n’en avez besoin que pour des outils réseau locaux, comme les lanceurs homebrew. La capture et l’entrée n’en ont jamais besoin.

Quelle que soit la variante choisie, la console n’a **aucun accès à Internet**. NS2Bench ne transfère jamais de trafic, n’active jamais le partage de connexion, le NAT ou un proxy, et n’interroge jamais de serveur DNS externe. Son DNS local ne répond qu’au test de connectivité de la console ; tous les autres noms sont refusés. Chaque requête de la console apparaît dans le journal **réseau**.

La liaison Bridge entre l’ordinateur et une carte Linux est séparée. Elle peut passer par votre réseau local habituel.

### A · Câble direct (macOS ou Linux)

**Matériel :** un adaptateur Ethernet supplémentaire sur l’ordinateur, réservé à la console, un câble Ethernet et le port LAN de la station d’accueil (ou un adaptateur USB-Ethernet sur la console).

1. Donnez à l’adaptateur dédié une adresse privée fixe, par exemple `192.168.80.1/24`. NS2Bench ne configure pas l’interface pour vous.
2. Décrivez cette interface dans `bench.json` (avec le nom d’interface de votre adaptateur) :
   ```json
   "network": { "interface": "en7", "address": "192.168.80.1", "subnet": "192.168.80.0/24",
                "pool_start": "192.168.80.10", "pool_end": "192.168.80.13" }
   ```
   Compilez le service réseau : `make netd`.
3. Vérifiez la configuration sans ouvrir de port : `native/target/release/ns2bench-netd --check bench.json`.
4. Installez-le comme service :
   - **macOS :** copiez le binaire dans `~/.ns2bench/bin/ns2bench-netd`, remplissez `launchd/ns2bench-netd.plist.template` et copiez-le dans `~/Library/LaunchAgents/`. Pas besoin de root.
   - **Linux :** installez-le dans `/opt/ns2bench/ns2bench-netd` et utilisez l’unité systemd `launchd/ns2bench-netd.service`.
5. Reliez l’Ethernet de la console à l’adaptateur dédié.
6. Dans **Réglages → Réseau**, choisissez **Direct et isolé : ordinateur ↔ console**. Activez au besoin le DHCP local, le DNS et la page de test de connectivité, puis appliquez.

La console reçoit une adresse du pool (par exemple `192.168.80.10`–`192.168.80.13`). L’onglet **Réseau** affiche l’état en direct et les baux.

Le service refuse de démarrer si le transfert IP est activé, ou s’il ne peut pas le vérifier. C’est voulu.

### B · Routeur, VLAN ou Wi-Fi isolé (tout système)

1. Configurez un routeur, un VLAN ou un réseau Wi-Fi pour la console avec **Internet bloqué, IPv6 compris**. Un réseau domestique normal n’est pas isolé.
2. Laissez le routeur distribuer les adresses (DHCP).
3. Connectez-y la console en Ethernet ou en Wi-Fi.
4. Dans **Réglages → Réseau**, choisissez **Routeur / VLAN / Wi-Fi isolé**. Cela désactive le DHCP et le DNS locaux de NS2Bench pour éviter tout conflit avec le routeur.

### C · Ordinateur Windows avec un assistant distant

Windows ne peut pas exécuter le service réseau localement. Faites-le tourner sur une carte ou un PC Linux, et pilotez-le depuis Windows.

1. Sur l’assistant Linux, installez `ns2bench-netd` comme dans la variante A. L’assistant gère l’Ethernet de la console.
2. Créez un jeton : `openssl rand -hex 16 > ~/.ns2bench/netd-control.token`. Copiez le même fichier sur l’ordinateur Windows.
3. Dans la configuration de l’assistant, ajoutez un point d’écoute de gestion sur une adresse privée qui n’est **pas** sur le réseau de la console :
   ```json
   "network": { "control": { "listen": "192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
4. Dans la configuration de NS2Bench sous Windows, indiquez l’assistant :
   ```json
   "network": { "helper": { "url": "http://192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
5. Gardez la liaison de gestion privée : un câble direct ou le réseau USB de la carte. Son trafic est en HTTP non chiffré.

L’onglet **Réseau** sous Windows affiche alors l’état et les commandes de l’assistant. Ou bien passez-vous de l’assistant et utilisez la variante B.

**Problèmes courants**

- *L’onglet Réseau indique « indisponible » :* le service ne tourne pas, ou lui et l’interface utilisent des dossiers d’état différents. Sous Windows, vérifiez `network.helper` et que les deux fichiers de jeton correspondent.
- *La console n’obtient pas d’adresse :* vérifiez le nom d’interface et le sous-réseau dans la configuration, et que le câble va bien à l’adaptateur dédié.
- *La console dit qu’elle n’a pas Internet :* c’est correct. Le test de connectivité réussit localement, mais rien ne va plus loin.

## Agents

Une fois les pastilles au vert, les agents peuvent utiliser le banc. Ouvrez **Réglages → API et MCP** pour l’adresse de l’API, des exemples de requêtes et une configuration MCP prête à copier pour Claude Code, Codex ou opencode. Agents et personnes partagent les mêmes commandes, et l’interrupteur « manuel uniquement » bloque à tout moment les entrées des agents.

## Notes pour Windows

- La capture utilise `ffmpeg` avec DirectShow. Fermez les autres applications qui utilisent la carte.
- La carte Linux, le Pico et le CH9329 fonctionnent tous depuis Windows. Les ports série s’appellent `COM3`, `COM4`, etc.
- Il n’y a pas de service réseau local sous Windows. Utilisez un assistant distant (variante C) ou un routeur isolé (variante B).
