# Général

## Qu’est-ce que NS2Bench ?

Un banc pour une Nintendo Switch 2 posée sur votre bureau. Il affiche l’image et le son de la console sur votre ordinateur et vous permet, à vous ou à un agent IA, d’appuyer sur ses boutons. Vous pouvez enregistrer des clips, lire le texte à l’écran et lancer des tests reproductibles.

## À qui s’adresse-t-il ?

Aux développeurs qui testent un logiciel sur du vrai matériel, aux personnes qui automatisent des vérifications répétitives, aux expériences d’accessibilité, et à tous ceux qui veulent qu’un agent IA utilise une console pendant qu’ils regardent.

## Modifie-t-il ma console ?

Non. NS2Bench fait seulement ce que font une télé et une manette filaire : il reçoit le HDMI et appuie sur des boutons via USB. Rien n’est installé sur la console.

## La console va-t-elle en ligne ?

Non. NS2Bench ne donne jamais accès à internet à la console. La capture et l’entrée USB n’ont besoin d’aucun réseau. Le réseau optionnel de la console répond seulement à son test de connexion et ne transmet jamais de trafic. Voir [Réseau hors ligne pour la console](#offline-console-network).

## Est-ce gratuit ?

Oui. NS2Bench est gratuit et open source sous licence GPL-3.0. Vous pouvez l’utiliser, le modifier et le partager. Quand vous le transmettez, modifié ou non, conservez la mention « NS2Bench, created by PhantomPtr » et la licence ; pour une version modifiée, proposez aussi son code source à ceux qui la reçoivent et indiquez qu’elle est modifiée (voir NOTICE). S’il vous est utile, vous pouvez [le soutenir](README.md#support).

## Est-ce affilié à Nintendo ?

Non. NS2Bench est un projet indépendant. Nintendo Switch est une marque de Nintendo.

# Installation

## De quoi ai-je besoin ?

Une carte de capture pour la vidéo et le son (toujours), et un appareil d’entrée si vous voulez appuyer sur des boutons :

- une carte Linux avec un port USB device (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5),
- ou un Raspberry Pi Pico avec un adaptateur USB-UART 3,3 V,
- ou un module CH9329 (clavier et souris uniquement).

Le [guide d’installation](docs/setup/README.md) contient un schéma et les étapes pour chacun.

## Que dois-je installer sur mon ordinateur ?

- **macOS :** rien ; autorisez simplement Caméra, Micro et Réseau local. `brew install ffmpeg` si vous voulez enregistrer des clips.
- **Windows :** ffmpeg (`winget install Gyan.FFmpeg`), et un pilote CH340/CP210x si votre adaptateur série en a besoin.
- **Linux :** ffmpeg (le `.deb` l’installe) et les groupes `video` et `dialout` (`sudo usermod -aG video,dialout $USER`).

Voir [Avant de commencer](docs/setup/README.md#before-you-start-your-computer).

## Quel appareil d’entrée choisir ?

Si vous voulez tous les boutons, y compris C, GL et GR, utilisez une carte Linux : son mode Pro présente à la console une vraie manette Pro Switch 2. Un Pico est l’option manette la moins chère (tous les boutons sauf C, GL et GR). Un CH9329 ne fournit qu’un clavier et une souris. Vous pouvez commencer en visualisation seule et ajouter un appareil d’entrée plus tard.

## Ai-je besoin de la station d’accueil ?

La console doit pouvoir sortir du HDMI, ce qui passe normalement par la station d’accueil. L’appareil d’entrée se branche sur l’un de ses ports USB.

## Puis-je l’utiliser sans appareil d’entrée ?

Oui. Choisissez **Visualisation seule (sans appareil d’entrée)** dans Réglages → Appareils. Vous pouvez regarder, écouter, faire des captures d’écran, enregistrer des clips et lire le texte à l’écran.

## Où régler la carte de capture ?

Dans le panneau **Vidéo** à droite de la fenêtre principale. Il ne liste que les formats proposés par votre carte.

## Où régler l’appareil d’entrée ?

Dans **Réglages → Appareils**. Les changements s’appliquent sans redémarrer ; l’ancienne connexion relâche d’abord tous les boutons.

## Comment savoir si ça marche ?

La pastille **vidéo** de la barre du haut passe au vert avec une fréquence d’images, et la pastille **Bridge** passe au vert quand la console a accepté l’appareil d’entrée. **Diagnostic** dans la barre du haut copie un résumé complet de l’état.

# Capture et son

## Quelles cartes de capture fonctionnent ?

Toute carte de capture HDMI USB (UVC). L’objectif est 1080p60. Testée : UGREEN 15389 (MS2130), en 1080p60 et 1440p30.

## Puis-je utiliser OBS en même temps ?

Sur macOS, oui : NS2Bench partage la carte avec OBS et ne la lui prend jamais. Sur Windows et Linux, ffmpeg a généralement besoin de la carte pour lui seul ; fermez donc les autres apps qui l’utilisent.

## Il n’y a pas d’image.

Vérifiez que la console est sur sa station et allumée, que le câble HDMI va dans l’**entrée** de la carte, et que la bonne carte est sélectionnée dans le panneau Vidéo. Sur macOS, autorisez la **Caméra** pour NS2Bench dans Réglages Système → Confidentialité et sécurité. Lancez **Diagnostic** pour plus de détails.

## Il n’y a pas de son.

Cliquez sur **Son coupé** dans la barre du haut pour activer le son. Sur macOS, autorisez le **Micro** pour NS2Bench. Si vous avez deux cartes de capture identiques, NS2Bench associe le son à l’image selon la position USB.

## Combien de CPU la capture utilise-t-elle ?

Sur macOS, environ 12 % d’un cœur en 1080p60 (JPEG et H.264 matériels). Sur Linux, le flux des cartes MJPEG est transmis sans réencodage.

# Appareils d’entrée

## La pastille Bridge reste rouge.

- **Carte Linux :** vérifiez que la carte est alimentée et joignable à l’adresse indiquée dans Réglages → Appareils, et que le fichier de jeton correspond au jeton de la carte.
- **Pico :** vérifiez le port série et que le débit est de 921600 bauds ; TX et RX doivent être croisés (TX de l’adaptateur → GP1 du Pico, RX de l’adaptateur → GP0 du Pico) et GND doit être relié.
- **CH9329 :** utilisez le débit du module (généralement 9600).

Vérifiez ensuite que l’appareil d’entrée est branché sur le port USB de la console et que la console est réveillée.

## Les appuis ne sont pas pris en compte.

La Switch 2 exige qu’un appui dure au moins 50 ms environ. NS2Bench maintient chaque appui au moins 75 ms. Si Bridge est vert mais que rien ne se passe, la console est peut-être en veille ou affiche un écran assombri : le premier appui ne fait que la réveiller.

## Comment obtenir C, GL et GR ?

GL et GR sont les deux boutons arrière de la manette Pro Nintendo Switch 2 ; les Joy-Con 2 n’en ont pas. Pour les utiliser (ainsi que C), utilisez une carte Linux et choisissez **La console voit → manette Pro** dans le panneau Pilotage. La console voit alors une manette Pro Switch 2. Sinon, C sert de raccourci GameChat.

## Puis-je utiliser ma propre manette ?

Oui. Branchez-la sur l’ordinateur ; NS2Bench l’associe selon la position des boutons (le bouton frontal du bas est B).

## Puis-je taper au clavier ?

Choisissez **Votre clavier envoie → Clavier et souris** dans le panneau Pilotage. La console voit un vrai clavier USB. Il faut une carte Linux, un Pico en mode standard ou un CH9329.

## Que se passe-t-il quand la console se met en veille ?

Une carte alimentée par l’USB de la console perd son alimentation et redémarre quand la console se réveille. NS2Bench se reconnecte tout seul. Les appuis faits pendant que Bridge est hors ligne sont abandonnés, jamais rejoués plus tard.

## NS2Bench peut-il sortir la console de veille ?

Pas encore. La Switch 2 n’a pas de Wake-on-LAN, et une carte alimentée par le port USB de la console s’éteint avec elle (testé : le Bridge se coupe environ 7 secondes après la mise en veille ; quand vous réveillez la console, la carte démarre et NS2Bench se reconnecte tout seul, prêt en 30 secondes environ). Deux pistes sont à l’étude : alimenter la carte séparément (un répartiteur d’alimentation USB qui ne renvoie pas de courant vers la console) pour qu’elle reste allumée, ou HDMI-CEC « Synchroniser avec l’état d’alimentation du téléviseur », où un appareil CEC côté HDMI réveille la console. En attendant, réveillez-la avec son bouton d’alimentation.

# Réseau hors ligne pour la console

## Ai-je besoin d’un réseau ?

Non. La capture et l’entrée USB fonctionnent sans réseau. Le réseau de la console sert uniquement aux outils qui dialoguent avec la console sur un LAN.

## Comment donner à la console un LAN sans internet ?

Reliez l’Ethernet de la console (port de la station ou adaptateur USB) à un adaptateur Ethernet dédié sur votre Mac ou ordinateur Linux et lancez `ns2bench-netd`. Il distribue les adresses et répond uniquement au test de connexion. Vous pouvez aussi utiliser un routeur, un VLAN ou un réseau Wi-Fi avec internet bloqué. Voir le [guide d’installation](docs/setup/README.md#optional-offline-console-network).

## Puis-je désactiver le blocage d’internet ?

Non. Ce n’est pas un réglage. NS2Bench ne transmet, ne NATe ni ne proxifie jamais le trafic de la console, et n’interroge jamais un serveur DNS en amont.

## Je suis sous Windows.

Le service réseau fonctionne sur macOS et Linux. Sous Windows, lancez-le sur une carte Linux ou un PC qui gère le port Ethernet de la console et pilotez-le depuis l’onglet Réseau ([assistant distant](docs/REFERENCE.md#remote-helper-windows-hosts)), ou utilisez un routeur avec internet bloqué.

# Agents et API

## Quels agents fonctionnent ?

Tout client MCP : Claude Code, Codex, opencode et d’autres. Réglages → API et MCP affiche une configuration prête à copier.

## Puis-je arrêter un agent ?

Oui. **Bloquer les agents (manuel uniquement)** dans le panneau Pilotage arrête immédiatement l’entrée des agents. Vous seul, depuis la fenêtre NS2Bench, pouvez le désactiver.

## L’API est-elle sûre sur mon réseau ?

Le moteur n’écoute que sur `127.0.0.1` par défaut. Écouter sur une autre adresse exige un jeton. Les pages web d’autres sites ne peuvent pas lui envoyer de commandes.

## Où l’API est-elle documentée ?

Dans [docs/REFERENCE.md](docs/REFERENCE.md#agents-mcp-and-http-api).

# App et mises à jour

## Comment fonctionnent les mises à jour ?

L’app vérifie les versions GitHub chaque jour et vous prévient quand une mise à jour est disponible. Chaque téléchargement est vérifié avec le SHA-256 de la version avant l’installation. Le téléchargement et l’installation automatiques sont désactivés par défaut ; activez-les dans Réglages → Mises à jour.

## macOS dit que l’app est endommagée ou ne peut pas être vérifiée.

L’app n’est pas encore notarisée. Lancez `xattr -dr com.apple.quarantine /Applications/NS2Bench.app` une fois, puis ouvrez-la.

## Windows dit qu’il a protégé mon PC.

Choisissez **Informations complémentaires → Exécuter quand même**. Le programme d’installation n’est pas encore signé avec un certificat payant.

## macOS redemande la Caméra ou le Micro après une mise à jour.

Tant que l’app n’est pas notarisée, chaque mise à jour compte comme une nouvelle app pour macOS, qui peut donc redemander. Autorisez-la encore une fois ; rien d’autre ne change.

## Comment changer le thème ou la langue ?

Réglages → Apparence. Le bouton lune dans la barre du haut bascule entre clair et sombre.

## Où sont mes clips, captures d’écran et journaux ?

Dans `~/.ns2bench/` (clips, rapports, journaux). Rien n’est envoyé nulle part.

# Dépannage

## Comment signaler un bug ?

Cliquez sur **Archive** dans la barre du haut pour enregistrer un zip de rapport de bug (journaux, état, une capture d’écran ; sans jetons) et joignez-le à une [issue GitHub](https://github.com/phantomptr/ns2bench/issues/new/choose).

## Que fait doctor ?

`ns2bench doctor` vérifie la vidéo, l’image de la console, l’OCR, le Bridge, l’état USB, la calibration et l’isolation réseau, puis indique quelle partie est en panne et quoi faire. `--probe` appuie aussi sur RIGHT et LEFT dans le menu HOME pour tester l’entrée de bout en bout.

## Où demander de l’aide ?

Sur [Discord](https://discord.gg/fzK3xddtrM).
