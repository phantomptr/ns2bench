# Configurar NS2Bench

NS2Bench te permite a ti, o a un agente de IA, ver, oír y controlar una Nintendo Switch 2 desde un ordenador.
Esta guía muestra cómo conectarlo todo.

![Vista general de la configuración de NS2Bench](overview.svg)

Toda configuración tiene dos partes independientes:

- **Ojos (siempre):** el HDMI de la consola entra en una capturadora USB. La capturadora le da al ordenador vídeo y audio.
- **Manos (opcional):** un pequeño dispositivo de entrada se conecta al puerto USB de la consola. El ordenador le envía órdenes. La consola ve un mando con cable, un teclado y un ratón.

Las personas usan la página de NS2Bench en un navegador o la app de escritorio. Los agentes usan la API HTTP o MCP en el mismo ordenador (`127.0.0.1:7630`). Todos comparten un mismo banco de pruebas, y ves cada acción de los agentes en directo.

**La consola sigue sin conexión.** NS2Bench nunca le da acceso a internet. La captura y la entrada USB no necesitan ninguna red.

## ¿Qué configuración necesito?

| Lo que tienes | Configuración | Conexión de entrada en Ajustes |
|---|---|---|
| Solo una capturadora | [Solo visualización](#view-only) | Solo visualización |
| Una placa Linux con modo dispositivo USB (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5) | [Placa Linux](#linux-usb-gadget-board) | Bridge de red |
| Una Raspberry Pi Pico (RP2040) y un adaptador USB a UART de 3,3 V | [Pico](#raspberry-pi-pico) | Bridge serie |
| Un módulo CH9329 (solo teclado y ratón) | [CH9329](#ch9329-module) | CH9329 |
| También quieres la consola en una red local (herramientas homebrew) | [Red sin conexión](#optional-offline-console-network) | (pestaña Red) |
| Varias consolas, cada una con su propia capturadora | [Varias consolas](#several-consoles) | (pestaña Consolas) |

¿No estás seguro? Empieza con **solo visualización** y añade un dispositivo de entrada más tarde. Los ajustes de entrada cambian sin reiniciar la app.

### Cómo se conecta un dispositivo de entrada

Un dispositivo de entrada necesita dos conexiones: **USB a la consola** (donde hace de mando) y **un enlace con este ordenador** (para que NS2Bench le diga qué pulsar).

| Dispositivo | A la consola | A este ordenador | Notas |
|---|---|---|---|
| **Placa Linux** (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5) · **recomendada** | su puerto USB de dispositivo | Ethernet o Wi-Fi, integrados | Una placa hace las dos cosas. Todos los botones, incluidos C, GL y GR en modo Pro. Se instala y actualiza desde la app por SSH |
| **Raspberry Pi Pico** (RP2040) | su único puerto USB | sus pines serie (GP0, GP1, GND) con un **adaptador USB a UART de 3.3 V** | La consola ocupa el puerto USB, así que el ordenador necesita el adaptador. Otra Pico con el firmware *debugprobe* de Raspberry Pi sirve como adaptador. El firmware de la Pico aún no se ha probado en una consola |
| **CH9329** | su lado USB | su lado serie, con un adaptador USB a UART | Solo teclado y ratón, sin botones de mando |

Si puedes, elige una **placa Linux como la Luckfox**: no necesita nada más, es la configuración probada en consolas reales y ofrece el modo Switch 2 Pro Controller. Algunas placas RP2040 baratas con puerto USB-C no las ve un Mac con un cable USB-C a USB-C; usa un cable USB-A a USB-C para flashearlas.

Si quieres todos los botones del mando, incluidos C, GL y GR, usa una placa Linux. Su modo **Pro** le muestra a la consola un Mando Pro de Switch 2. La Pico y el modo estándar de la placa cubren todos los botones excepto C, GL y GR. CH9329 no tiene mando.

## Antes de empezar: tu ordenador

Instala la app desde la [última versión](https://github.com/phantomptr/ns2bench/releases/latest) y luego:

### macOS (14 o posterior)

- **No hay que instalar nada más.** La captura, el sonido y la lectura de texto en pantalla vienen integrados en la app.
- Primer inicio: si macOS dice que Apple no ha podido verificar el .dmg, o que la app está dañada o no se puede comprobar, ejecuta `xattr -dr com.apple.quarantine ~/Downloads/NS2Bench-*.dmg` antes de abrir el .dmg (o `xattr -dr com.apple.quarantine /Applications/NS2Bench.app` una vez instalada), o haz clic en **Abrir igualmente** en Ajustes del Sistema → Privacidad y seguridad. Solo en la primera instalación: las actualizaciones desde la app no lo necesitan.
- Permite **Cámara** (la imagen), **Micrófono** (el sonido de la consola) y **Red local** (el dispositivo de entrada) cuando se te pida. Puedes cambiarlos más tarde en Ajustes del Sistema → Privacidad y seguridad.
- **Clips** (Guardar clip) necesitan ffmpeg: `brew install ffmpeg`.
- Los adaptadores serie (Pico, CH9329) funcionan sin controladores; aparecen como `/dev/cu.usbserial-…` o `/dev/cu.usbmodem…`.

### Windows 10/11

- **Instala ffmpeg** (la captura, el sonido y los clips lo usan): `winget install Gyan.FFmpeg` y luego reinicia NS2Bench. NS2Bench también encuentra ffmpeg instalado con Chocolatey o Scoop, o un `ffmpeg.exe` junto a `ns2bench-app.exe`.
- SmartScreen puede decir "Windows protegió su PC": elige **Más información → Ejecutar de todas formas**.
- Los adaptadores USB a UART pueden necesitar el controlador del fabricante (CH340/CH341 o CP210x). El puerto aparece como `COM3` o similar en el Administrador de dispositivos.
- Opcional: [Tesseract](https://github.com/UB-Mannheim/tesseract/wiki) para leer texto en pantalla.
- El servicio de red sin conexión para la consola no funciona en Windows: usa un [ayudante Linux o un router](#optional-offline-console-network).

### Linux

- **ffmpeg:** el `.deb` lo instala por ti. Con la AppImage, instálalo tú (`sudo apt install ffmpeg`, `sudo dnf install ffmpeg`, `sudo pacman -S ffmpeg`).
- **Permisos:** añádete a los grupos dueños de las capturadoras y los puertos serie, y luego cierra sesión y vuelve a entrar:
  `sudo usermod -aG video,dialout $USER` (Arch: `video,uucp`).
- AppImage: `chmod +x NS2Bench-*.AppImage`. Si no arranca, instala FUSE 2 (`sudo apt install libfuse2`).
- Opcional: `sudo apt install tesseract-ocr` para leer texto en pantalla.
- Los puertos serie aparecen como `/dev/ttyACM0` (Pico) o `/dev/ttyUSB0` (la mayoría de adaptadores).

## La capturadora (en todas las configuraciones)

- Sirve cualquier capturadora HDMI USB **UVC**. El objetivo es 1080p60. Probada: UGREEN 15389 (MS2130).
- Conecta la **salida HDMI** de la base de la consola a la entrada HDMI de la capturadora. Conecta la capturadora al ordenador por USB.
- Elige la capturadora, la resolución, la tasa de fotogramas y la calidad en el panel **Vídeo**. Solo muestra lo que ofrece tu capturadora.
- El **audio** también llega por la capturadora. No hay un cable de audio aparte.
- **macOS:** un pequeño ayudante integrado hace la captura. Comparte la capturadora con OBS: si OBS ya la está usando, NS2Bench usa el formato de OBS y nunca le quita la capturadora. En el primer inicio, permite **Cámara** (imagen), **Micrófono** (audio de la consola) y **Red local** (el Bridge).
- **Linux y Windows:** NS2Bench usa `ffmpeg` (instálalo primero). Puede que la capturadora tenga que estar libre, así que cierra OBS u otras apps que la estén usando si la imagen no aparece.
- Ejecuta `ns2bench devices` para ver las capturadoras que NS2Bench detecta.

## Solo visualización

![Solo visualización: una capturadora, sin dispositivo de entrada](view-only.svg)

El banco de pruebas más sencillo. Puedes ver en directo, grabar, hacer capturas a resolución completa y leer texto en pantalla (OCR). Los agentes también pueden ver, pero nadie puede pulsar botones.

**Piezas**

- La consola en su base
- Capturadora HDMI USB (UVC)
- Cable HDMI y el cable USB de la capturadora

**Pasos**

1. Conecta la salida HDMI de la base a la capturadora.
2. Conecta la capturadora al ordenador.
3. Inicia NS2Bench y abre `http://127.0.0.1:7630` (o la app de escritorio).
4. Elige tu capturadora en el panel **Vídeo**.
5. En **Ajustes → Dispositivos**, pon **Conexión de entrada** en **Solo visualización** y haz clic en **Aplicar conexión de entrada**.

**Comprueba que funciona**

- La píldora de **vídeo** de la barra superior se pone verde y aparece la imagen de la consola.
- La píldora del **Bridge** dice *not configured*. Es lo esperado aquí.

**Problemas habituales**

- *Imagen negra:* despierta la consola; asegúrate de que está en la base y de que la base tiene corriente. Prueba otro cable HDMI.
- *La capturadora no aparece en la lista:* en macOS, permite el acceso a la Cámara. En Linux o Windows, cierra cualquier app que esté usando la capturadora.
- *Sin sonido:* en macOS, permite el acceso al Micrófono. Comprueba que la capturadora es la fuente de audio seleccionada.

## Placa Linux con USB gadget

![Placa Linux: USB a la consola, Ethernet al ordenador](input-linux-board.svg)

Una pequeña placa Linux se hace pasar por dispositivos USB. Ejecuta **NS2Bench Bridge** (`ns2bench-board`). Esta es la configuración probada (Luckfox Pico Pro/Max). Admite los dos modos de mando:

- **Estándar** (predeterminado): la consola ve un mando con cable estilo HORI + teclado USB + ratón USB.
- **Pro**: la consola ve un Mando Pro de Switch 2, con todos los botones, incluidos C, GL y GR. En este modo no hay teclado ni ratón.

Cambia de modo con **La consola ve** en el panel Controlando. Tarda unos 8 segundos.

**Piezas**

- Una placa Linux con modo dispositivo USB (OTG): Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, o Pi 4/5 usando el puerto USB-C
- Un cable USB desde el puerto **OTG** de la placa al puerto USB de la base
- Un cable Ethernet al ordenador (o usa Wi-Fi o la red USB de la placa)
- La configuración de la capturadora de arriba

**Pasos**

1. Instala NS2Bench Bridge en la placa desde la app: **Ajustes → Instalar → Placa Bridge**, escribe su host SSH y haz clic en **Instalar en la placa**. (Desde el código fuente: `make board-install BOARD=<ssh-host>`; añade `BOARD_TARGET=aarch64-unknown-linux-musl` para una Raspberry Pi de 64 bits.)
2. Conecta el puerto USB **OTG** de la placa al puerto USB de la base.
3. Conecta el Ethernet de la placa al ordenador (o pon ambos en la misma red privada).
4. Inicia el Bridge si aún no está en marcha: `ssh <ssh-host> /opt/ns2bench/ns2bench-hidd.sh start --bind <board-ip>`.
5. Asegúrate de que el archivo de token `~/.ns2bench/hidd.token` del ordenador coincide con el token de la placa.
6. En **Ajustes → Dispositivos** configura:
   - **Conexión de entrada:** Bridge de red
   - **Dirección del Bridge:** la IP de la placa (por defecto `192.168.79.2`)
   - **Puerto del Bridge:** `7631`
   - **Archivo de token del Bridge:** `~/.ns2bench/hidd.token`
7. Haz clic en **Aplicar conexión de entrada**.

**Comprueba que funciona**

- La píldora del **Bridge** se pone verde y muestra el tiempo de ida y vuelta y *usb configured*.
- Pulsa un botón del Joy-Con en pantalla. La consola reacciona y el botón se ilumina.

**Cuando la consola entra en reposo**

Una placa alimentada por el USB de la consola se queda sin corriente cuando la consola entra en reposo, y arranca de nuevo cuando despierta. Deja NS2Bench en marcha: lo detecta en unos tres segundos y se vuelve a conectar solo. La entrada vuelve a funcionar cuando la placa ha arrancado y la consola ha configurado su USB.

Para que esto funcione, la placa debe levantarse sola al arrancar. Añade un script de arranque para tu placa que ejecute `/opt/ns2bench/gadget.sh` y después `/opt/ns2bench/ns2bench-hidd.sh start`. Prueba un reposo y un despertar reales con tu placa y tu base.

**Problemas habituales**

- *Bridge offline · reconnecting:* revisa el cable y la IP de la placa. Haz ping a la placa. Asegúrate de que el Bridge está en marcha en ella.
- *Bridge conectado pero usb no está configurado:* despierta la consola. Comprueba que el cable va al puerto **OTG** de la placa, no a un puerto solo de alimentación. Algunos cables solo llevan corriente.
- *El ratón no hace nada en el menú HOME:* es normal. La consola lee el ratón, pero solo algunos programas muestran un cursor.
- *Teclado ignorado:* la compatibilidad con teclados USB de la consola está en Configuración de la consola → Mandos y accesorios.
- En la placa, para e inicia las cosas solo con `ns2bench-hidd.sh` y `gadget.sh`. No mates procesos por patrón.

## Raspberry Pi Pico

![Raspberry Pi Pico: USB a la consola, UART a un adaptador USB en el ordenador](input-pico.svg)

Una Raspberry Pi Pico (RP2040) con el firmware NS2Bench Bridge. Su puerto USB va a la consola y muestra el mismo mando + teclado + ratón que una placa Linux en modo estándar. El ordenador se comunica con ella por un enlace serie (UART) a través de un adaptador USB a UART aparte.

> El firmware de la Pico está compilado, pero aún no se ha probado en hardware real. Se agradecen los informes.

**Piezas**

- Raspberry Pi Pico (RP2040) y un cable micro-USB
- Un adaptador USB a UART de **3,3 V** (CP2102, FT232 o CH340)
- Tres cables de puente
- La configuración de la capturadora de arriba

**Pasos**

1. Flashea la Pico desde la app: **Ajustes → Instalar → Flashear Pico** y mantén pulsado el botón **BOOTSEL** mientras conectas la Pico al **ordenador**. La app copia el firmware en la unidad **RPI-RP2** y la Pico se reinicia. Desconéctala. (Desde el código fuente: `make pico` compila `firmware/pico/ns2bench-pico.uf2`; cópialo tú mismo en la unidad RPI-RP2.)
2. Conecta la Pico al adaptador:
   - Pico **GP0** (pin 1, UART0 TX) → adaptador **RX**
   - Pico **GP1** (pin 2, UART0 RX) ← adaptador **TX**
   - Pico **GND** (pin 3) — adaptador **GND**
   - Deja sin conectar el pin de 3,3 V / 5 V del adaptador. La Pico recibe corriente de la consola.
3. Conecta el micro-USB de la Pico al puerto USB de la base.
4. Conecta el adaptador al ordenador.
5. En **Ajustes → Dispositivos** configura:
   - **Conexión de entrada:** Bridge serie
   - **Puerto serie:** el adaptador (por ejemplo `/dev/cu.usbserial-XXXX` en macOS, `/dev/ttyUSB0` en Linux, `COM3` en Windows)
   - **Velocidad en baudios:** `921600`
6. Haz clic en **Aplicar conexión de entrada**.

Opcional: compila con `NS2BENCH_TOKEN=... make pico` para que la Pico exija un token (configura el mismo archivo de token en el ordenador). Compila con `NS2BENCH_BAUD=115200` si tu adaptador no admite 921600.

**Comprueba que funciona**

- La píldora del **Bridge** se pone verde y muestra *usb configured*.
- Una pulsación del Joy-Con en pantalla llega a la consola.

**Problemas habituales**

- *No aparece ningún puerto serie:* instala el controlador del adaptador (CP210x, FTDI o CH340) y vuelve a conectarlo.
- *Bridge offline:* probablemente TX y RX están intercambiados. TX siempre va a RX. Revisa también GND.
- *Respuestas ilegibles o ninguna respuesta:* la velocidad en baudios debe coincidir con la del firmware (921600, salvo que lo compilaras con otro valor).
- *Nunca uses un adaptador de 5 V.* Los pines de la Pico son solo de 3,3 V.

## Módulo CH9329

![CH9329: USB a la consola, UART al ordenador](input-ch9329.svg)

Un módulo CH9329 convierte órdenes serie en un teclado y un ratón USB. Es barato y sencillo, pero **no tiene mando**: ni botones ni palancas. Úsalo solo para escribir y apuntar.

**Piezas**

- Un módulo CH9329
- Un adaptador USB a UART de 3,3 V, salvo que el cable de tu módulo ya lleve USB-serie integrado
- La configuración de la capturadora de arriba

**Pasos**

1. Conecta el lado USB (HID) del módulo al puerto USB de la base.
2. Conecta el lado UART al adaptador: TX del módulo → RX del adaptador, RX del módulo ← TX del adaptador, GND — GND. (Sáltate esto si el módulo tiene USB-serie integrado.)
3. Conecta el adaptador (o el cable serie del módulo) al ordenador.
4. En **Ajustes → Dispositivos** configura:
   - **Conexión de entrada:** CH9329
   - **Puerto serie:** el puerto del adaptador
   - **Velocidad en baudios:** la del módulo, normalmente `9600`
5. Haz clic en **Aplicar conexión de entrada**.

**Comprueba que funciona**

- La píldora del **Bridge** se pone verde.
- Lo que escribes en el modo de teclado nativo aparece en la consola (por ejemplo, en un campo de texto).

**Problemas habituales**

- *Los botones del mando no hacen nada:* es lo esperado. CH9329 es solo teclado y ratón.
- *No pasa nada:* revisa la velocidad en baudios e intercambia TX/RX si hace falta.
- *La consola ignora el teclado:* revisa Configuración de la consola → Mandos y accesorios → Teclado USB.

## Varias consolas

![Dos consolas, cada una con su propia capturadora y dirección de red, que comparten un mando cuyo cable USB se cambia de una a otra](multi-console.svg)

Un mismo banco puede manejar varias consolas a la vez. Cada una tiene su propia **capturadora** (imagen y sonido) y, si usas la red sin conexión, su propia **dirección de red**. Pueden compartir un **mando** (una placa Luckfox o Raspberry Pi, una Pico o un CH9329): su cable USB va conectado a una sola consola a la vez, y tú le dices a NS2Bench a cuál.

**Los nombres que usa NS2Bench**

| Nombre | Qué es |
|---|---|
| Consola | Una Switch, con el nombre que elijas y su modelo |
| Modelo | Switch 2 o Switch 1: define las rutas de menú, los nombres de pantalla y el modo del mando |
| Capturadora | La tarjeta HDMI a USB para la imagen y el sonido de esa consola |
| Mando | El dispositivo que pulsa los botones (el NS2Bench Bridge en una placa o Pico, o un CH9329) |
| Red | Cómo llega la consola a este ordenador (red de casa compartida, cable directo o ninguna) |

**Configuración**

1. Conecta el dock de cada consola a su propia capturadora, y las dos capturadoras al ordenador.
2. Abre **Ajustes → Consolas**. Tu primera consola ya está ahí: ponle un nombre y un modelo.
3. En **Añadir una consola**, escribe un nombre (por ejemplo *Switch 1*), elige su modelo, su capturadora (no se pueden elegir las que usa otra consola) y, si está en la red de casa compartida, su dirección. Haz clic en **Añadir consola**.
4. Debajo de la consola aparece una fila de **imágenes pequeñas en directo**: una por consola, con su nombre, su color y si su imagen está en directo. Haz clic en una para ver esa consola en grande. Las capturas, los clips, la lectura de pantalla y las pruebas funcionan entonces con ella. **+ Añadir consola** abre **Ajustes → Consolas**.

**El mando**

- 🎮 marca la consola a la que está conectado el mando. Los botones, las teclas y la entrada de agentes solo van ahí.
- **El mando va a la consola que elijas.** Haz clic en la imagen de una consola y pasa a ella el cable del mando: NS2Bench suelta todo lo pulsado y prepara la placa para esa consola. Recuerda tu elección, también tras reiniciar la placa o NS2Bench. (Se puede desactivar en Ajustes → Consolas.)
- **Cada consola tiene su propia elección de mando** (Ajustes → Consolas → Mando, o la tarjeta Mando): **Mando Pro**, el propio de ese tipo de consola (Switch 1 o Switch 2), o **Mando + teclado** para escribir.
- **Red de seguridad:** si una Switch 2 inicia la placa como su Mando Pro mientras hay otra consola elegida, NS2Bench sabe que el cable está en esa Switch 2, lo dice y la sigue. No se pulsa nada en la consola equivocada.

**Los agentes y scripts** indican la consola con `?console=<id or name>` o una cabecera `X-NS2Bench-Console`; las herramientas MCP aceptan un argumento `console`, y `list_consoles` las muestra todas. Sin indicarla, la petición va a la consola donde está el mando. La entrada para una consola sin el mando se rechaza con un mensaje que dice dónde está. Consulta la [referencia](../REFERENCE.md#agents-mcp-and-http-api).

**Todavía no:** compartir una capturadora entre consolas (un conmutador HDMI) ni varios mandos a la vez. Por ahora cada consola necesita su propia capturadora.

## Opcional: red sin conexión para la consola

![Tres formas de dar a la consola una red sin conexión](network.svg)

Solo lo necesitas para herramientas de red local, como cargadores homebrew. La captura y la entrada nunca lo necesitan. Las variantes A–C usan una red aparte para la consola. La variante D la mantiene en tu Wi-Fi de casa.

Elijas la variante que elijas, la consola **no tiene internet**. NS2Bench nunca reenvía tráfico, nunca activa la compartición de internet, NAT ni un proxy, y nunca consulta un servidor DNS externo. Su DNS local solo responde a la prueba de conexión de la consola; cualquier otro nombre se rechaza. Cada consulta que intenta la consola aparece en el registro de **red**.

El enlace del Bridge entre el ordenador y una placa Linux es independiente. Puede usar tu LAN normal.

### A · Cable directo (macOS o Linux)

**Piezas:** un adaptador Ethernet de sobra en el ordenador solo para la consola, un cable Ethernet y el puerto LAN de la base (o un adaptador USB-Ethernet en la consola).

1. Dale al adaptador dedicado una dirección privada fija, por ejemplo `192.168.80.1/24`. NS2Bench no configura la interfaz por ti.
2. Describe esa interfaz en `bench.json` (usa el nombre de interfaz de tu adaptador):
   ```json
   "network": { "interface": "en7", "address": "192.168.80.1", "subnet": "192.168.80.0/24",
                "pool_start": "192.168.80.10", "pool_end": "192.168.80.13" }
   ```
   Compila el servicio de red: `make netd`.
3. Comprueba la configuración sin abrir ningún puerto: `native/target/release/ns2bench-netd --check bench.json`.
4. Instálalo como servicio:
   - **macOS:** copia el binario a `~/.ns2bench/bin/ns2bench-netd`, rellena `launchd/ns2bench-netd.plist.template` y cópialo a `~/Library/LaunchAgents/`. No hace falta root.
   - **Linux:** instálalo en `/opt/ns2bench/ns2bench-netd` y usa la unidad systemd `launchd/ns2bench-netd.service`.
5. Conecta el Ethernet de la consola al adaptador dedicado.
6. En **Ajustes → Red**, elige **Directo y aislado: equipo ↔ consola**. Activa el DHCP local, el DNS y la página de prueba de conexión según lo necesites, y aplica.

La consola obtiene una dirección del rango (por ejemplo `192.168.80.10`–`192.168.80.13`). La pestaña **Red** muestra el estado en directo y las concesiones.

El servicio se niega a arrancar si el reenvío de IP está activado o si no puede comprobarlo. Es a propósito.

### B · Router, VLAN o Wi-Fi aislados (cualquier sistema)

1. Prepara un router, una VLAN o una red Wi-Fi para la consola con **internet bloqueado, incluido IPv6**. Una LAN doméstica normal no está aislada.
2. Deja que el router reparta las direcciones (DHCP).
3. Conecta la consola por Ethernet o Wi-Fi.
4. En **Ajustes → Red**, elige **Router / VLAN / Wi-Fi aislado**. Esto desactiva el DHCP y el DNS locales de NS2Bench para que no choquen con el router.

### C · Ordenador Windows con un ayudante remoto

Windows no puede ejecutar el servicio de red localmente. Ejecútalo en una placa o PC Linux y contrólalo desde Windows.

1. En el ayudante Linux, configura `ns2bench-netd` como en la variante A. El ayudante gestiona el Ethernet de la consola.
2. Crea un token: `openssl rand -hex 16 > ~/.ns2bench/netd-control.token`. Copia el mismo archivo al ordenador Windows.
3. En la configuración del ayudante, añade un punto de escucha de gestión en una dirección privada que **no** esté en la red de la consola:
   ```json
   "network": { "control": { "listen": "192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
4. En la configuración de NS2Bench en Windows, apunta al ayudante:
   ```json
   "network": { "helper": { "url": "http://192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
5. Mantén privado el enlace de gestión: un cable directo o la red USB de la placa. Su tráfico es HTTP sin cifrar.

La pestaña **Red** en Windows muestra entonces el estado y los controles del ayudante. O sáltate el ayudante y usa la variante B.

### D · Red doméstica compartida (Wi-Fi, sin hardware extra)

![Consolas en el Wi-Fi de casa sin internet: su puerta de enlace es su propia dirección y el ordenador solo responde a su prueba de conexión](shared-network.svg)

Las consolas siguen en tu Wi-Fi de casa de siempre, junto a tu ordenador. No hay que comprar ni conectar nada más. Cada consola usa **ajustes manuales** que no le dejan ningún sitio al que enviar tráfico, y NS2Bench en tu ordenador solo responde a su prueba de conexión, así que la consola sigue diciendo que está conectada.

**Cómo se bloquea internet**

| Vía de salida | Qué la bloquea |
|---|---|
| Buscar un nombre de Nintendo (eShop, actualizaciones, cuentas, telemetría) | El DNS de NS2Bench rechaza todos los nombres excepto la prueba de conexión. Cada intento aparece en la vigilancia de fugas y en el registro de **red**. |
| Conectarse a un servidor por dirección IP | La puerta de enlace de la consola es **su propia dirección**, así que nada de lo que envía sale de tu red de casa, sea cual sea la dirección. |
| IPv6 | Tu router no debe repartir IPv6, o desactívalo. |
| Tu ordenador reenvía el tráfico | La consola nunca envía tráfico a tu ordenador como puerta de enlace, y el asistente se detiene si se activa el reenvío o la compartición de internet. |
| La consola vuelve a los ajustes automáticos (un restablecimiento, una actualización, alguien los cambia) | La **vigilancia de fugas** te avisa en unos 10 minutos. Añade también una pausa en el router (paso 7). |

La última fila es lo único que una red compartida no puede descartar: ahí, los propios ajustes de la consola son el candado. Si necesitas "sin conexión incluso tras restablecer los valores de fábrica", usa la variante A o B, donde no existe ninguna ruta a internet.

**Necesitas:** NS2Bench en macOS o Linux con el asistente de red instalado, tu ordenador y las consolas en la misma red de casa, y un router que no reparta IPv6 (o con IPv6 desactivado).

1. **Reserva direcciones en tu router** (Google Home, la app de tu router o su página web): una para tu ordenador y una para cada consola. Anótalas, por ejemplo ordenador `192.168.86.199`, consolas `192.168.86.251` y `192.168.86.252`.
2. **Instala el asistente de red** si aún no lo has hecho: **Ajustes → Instalar → Asistente de red**.
3. Abre **Ajustes → Red** y elige **Red doméstica compartida**.
4. **Elige el adaptador de red** que está en la red de las consolas. La lista muestra cada adaptador con su dirección, por ejemplo `en10 · Belkin USB-C LAN · 192.168.86.199/24`.
5. Escribe las **direcciones de las consolas**, marca las tres casillas (sin IPv6, dirección del ordenador reservada, ningún otro Wi-Fi guardado en las consolas) y haz clic en **Guardar y usar esta red**. NS2Bench guarda una copia de seguridad de tu archivo de ajustes. Después haz clic en **Reiniciar asistente de red**.
6. **En cada consola:** Configuración de la consola → Internet → tu Wi-Fi → **Cambiar configuración**:
   - Configuración de dirección IP: **Manual**. Dirección IP: la dirección reservada de la consola. Máscara de subred: `255.255.255.0`. **Puerta de enlace: la propia dirección de la consola.**
   - Configuración de DNS: **Manual**. DNS primario y secundario: **la dirección de tu ordenador**.
   - Guarda y haz la prueba de conexión. Se supera y la consola dice que está conectada.
   NS2Bench muestra estos valores exactos para cada consola en **Escribe esto en cada consola**.
7. **Segundo candado recomendado:** en la app de tu router, pausa o bloquea internet para las consolas (Google Home: Wi-Fi → Family Wi-Fi → pausar). Comprueba que la prueba de conexión se sigue superando con la pausa activada.
8. Borra todas las demás redes Wi-Fi guardadas en cada consola y evita el juego inalámbrico local con una consola con un firmware más nuevo (las consolas pueden pasarse actualizaciones del sistema entre sí de esa forma).

**Cómo leer la vigilancia de fugas** (Ajustes → Red, se comprueba cada 30 segundos):

- **Protegida:** la consola ha preguntado a NS2Bench hace poco. Se muestra cuándo, su última prueba de conexión y cuántos nombres se rechazaron.
- **En tu red pero sin usar NS2Bench:** lleva 10 minutos sin preguntar mientras está en la red. Puede haber vuelto a los ajustes automáticos y estar en línea. Revisa sus ajustes. NS2Bench también envía una notificación de escritorio.
- **No está en la red ahora mismo:** apagada, en reposo o fuera. No hay nada que hacer.

**Problemas habituales**

- *La consola no se conecta:* comprueba que su DNS es la dirección de tu ordenador, que el asistente está en marcha (Ajustes → Instalar) y que el adaptador elegido en el paso 4 está en la red de las consolas. Solo reciben respuesta las direcciones de la lista, así que revisa también la IP de la consola.
- *La consola rechaza su propia dirección como puerta de enlace:* usa en su lugar una dirección libre fuera del rango DHCP del router, por ejemplo `192.168.86.254`. Asegúrate de que nada la use, ni ahora ni más adelante.
- *La dirección de tu ordenador ha cambiado:* las consolas no pueden conectarse (se quedan sin conexión). Resérvala en el router y vuelve a hacer la configuración.
- *Solo una red a la vez:* el asistente sirve esta red compartida o un cable directo (variante A), no las dos.

**Problemas habituales**

- *La pestaña Red dice que no está disponible:* el servicio no está en marcha, o el servicio y la interfaz usan carpetas de estado distintas. En Windows, revisa `network.helper` y que ambos archivos de token coincidan.
- *La consola no obtiene dirección:* revisa el nombre de interfaz y la subred en la configuración, y que el cable va al adaptador dedicado.
- *La consola dice que no tiene internet:* es correcto. La prueba de conexión se supera localmente, pero nada va más allá.

## Agentes

Cuando las píldoras estén en verde, los agentes pueden usar el banco de pruebas. Abre **Ajustes → API y MCP** para ver la dirección de la API, ejemplos de peticiones y una configuración MCP lista para copiar para Claude Code, Codex u opencode. Agentes y personas comparten los mismos controles, y el interruptor de solo manual bloquea la entrada de los agentes en cualquier momento.

## Notas para Windows

- La captura usa `ffmpeg` con DirectShow. Cierra otras apps que estén usando la capturadora.
- La placa Linux, la Pico y CH9329 funcionan desde Windows. Los puertos serie se llaman `COM3`, `COM4`, etc.
- No hay servicio de red local en Windows. Usa un ayudante remoto (variante C) o un router aislado (variante B).
