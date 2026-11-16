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

¿No estás seguro? Empieza con **solo visualización** y añade un dispositivo de entrada más tarde. Los ajustes de entrada cambian sin reiniciar la app.

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

## Opcional: red sin conexión para la consola

![Tres formas de dar a la consola una red sin conexión](network.svg)

Solo lo necesitas para herramientas de red local, como cargadores homebrew. La captura y la entrada nunca lo necesitan.

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
