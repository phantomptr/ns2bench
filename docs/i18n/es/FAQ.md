# General

## ¿Qué es NS2Bench?

Un banco de pruebas para una Nintendo Switch 2 en tu escritorio. Muestra la imagen y el sonido de la consola en tu ordenador y te permite, a ti o a un agente de IA, pulsar sus botones. Puedes grabar clips, leer texto en pantalla y ejecutar pruebas repetibles.

## ¿Para quién es?

Para desarrolladores que prueban software en hardware real, personas que automatizan comprobaciones repetitivas, experimentos de accesibilidad y cualquiera que quiera que un agente de IA maneje una consola mientras observa.

## ¿Modifica mi consola?

No. NS2Bench solo hace lo que hacen un televisor y un mando con cable: recibe HDMI y pulsa botones por USB. No se instala nada en la consola. Funciona con cualquier Switch 2 tal como viene de fábrica, con cualquier versión del sistema: sin jailbreak, sin modificaciones y sin homebrew.

## ¿La consola se conecta a internet?

No. NS2Bench nunca da acceso a internet a la consola. La captura y la entrada USB no necesitan ninguna red. La red opcional para la consola solo responde a su prueba de conexión y nunca reenvía tráfico. Consulta [Red sin conexión para la consola](#offline-console-network).

## ¿Es gratis?

Sí. NS2Bench es gratuito y de código abierto con licencia GPL-3.0. Puedes usarlo, modificarlo y compartirlo. Al distribuirlo, modificado o no, conserva el crédito "NS2Bench, created by PhantomPtr" y la licencia; si distribuyes una versión modificada, ofrece su código fuente a quienes la reciban e indica que está modificada (consulta NOTICE). Si te resulta útil, puedes [apoyarlo](README.md#support).

## ¿Está afiliado a Nintendo?

No. NS2Bench es un proyecto independiente. Nintendo Switch es una marca comercial de Nintendo.

# Configuración

## ¿Qué necesito?

Una capturadora para vídeo y sonido (siempre) y un dispositivo de entrada si quieres pulsar botones:

- una placa Linux con puerto USB de dispositivo (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5),
- o una Raspberry Pi Pico con un adaptador USB a UART de 3,3 V,
- o un módulo CH9329 (solo teclado y ratón).

La [guía de configuración](docs/setup/README.md) tiene un diagrama y los pasos para cada uno.

## ¿Qué tengo que instalar en mi ordenador?

- **macOS:** nada; solo permite Cámara, Micrófono y Red local. `brew install ffmpeg` si quieres guardar clips.
- **Windows:** ffmpeg (`winget install Gyan.FFmpeg`) y un controlador CH340/CP210x si tu adaptador serie lo necesita.
- **Linux:** ffmpeg (el `.deb` lo instala) y los grupos `video` y `dialout` (`sudo usermod -aG video,dialout $USER`).

Consulta [Antes de empezar](docs/setup/README.md#before-you-start-your-computer).

## ¿Qué dispositivo de entrada elijo?

Si quieres todos los botones, incluidos C, GL y GR, usa una placa Linux: su modo Pro muestra a la consola un Mando Pro de Switch 2 real. Una Pico es la opción de mando más barata (todos los botones excepto C, GL y GR). Un CH9329 solo ofrece teclado y ratón. Puedes empezar solo viendo y añadir un dispositivo de entrada más adelante.

## ¿Necesito la base?

La consola necesita una forma de sacar HDMI, que normalmente es la base. El dispositivo de entrada se conecta a uno de los puertos USB de la base.

## ¿Puedo usarlo sin dispositivo de entrada?

Sí. Elige **Solo visualización (sin dispositivo de entrada)** en Ajustes → Dispositivos. Puedes ver, escuchar, hacer capturas, guardar clips y leer texto en pantalla.

## ¿Dónde configuro la capturadora?

En el panel **Vídeo** a la derecha de la ventana principal. Solo muestra los formatos que ofrece tu capturadora.

## ¿Dónde configuro el dispositivo de entrada?

En **Ajustes → Dispositivos**. Los cambios se aplican sin reiniciar; la conexión anterior suelta primero todos los botones.

## ¿Puede la app instalar Bridge, el firmware de la Pico o el ayudante de red?

Sí, en **Ajustes → Instalar**. **Flashear Pico** pone el firmware de Bridge en una Raspberry Pi Pico (mantén pulsado BOOTSEL mientras la conectas). **Instalar en la placa** instala o actualiza Bridge en una placa Linux por SSH (configura antes el inicio de sesión con clave). **Instalar ayudante** ejecuta el ayudante de red sin conexión como servicio en macOS o Linux. Las descargas vienen de la versión que corresponde a la app y se comprueban antes de usarse.

## ¿Cómo sé que funciona?

La etiqueta **vídeo** de la barra superior se pone verde con la tasa de fotogramas, y la etiqueta **Bridge** se pone verde cuando la consola ha aceptado el dispositivo de entrada. **Diagnóstico** en la barra superior copia un resumen completo del estado.

# Captura y sonido

## ¿Qué capturadoras funcionan?

Cualquier capturadora HDMI USB (UVC). El objetivo es 1080p60. Probada: UGREEN 15389 (MS2130), a 1080p60 y 1440p30.

## ¿Puedo usar OBS a la vez?

En macOS, sí: NS2Bench comparte la capturadora con OBS y nunca se la quita. En Windows y Linux, ffmpeg suele necesitar la capturadora en exclusiva, así que cierra otras apps que la estén usando.

## No hay imagen.

Comprueba que la consola está en la base y encendida, que el cable HDMI va a la **entrada** de la capturadora y que está seleccionada la capturadora correcta en el panel Vídeo. En macOS, permite **Cámara** para NS2Bench en Ajustes del Sistema → Privacidad y seguridad. Ejecuta **Diagnóstico** para ver detalles.

## No hay sonido.

Haz clic en **Sonido desactivado** en la barra superior para activar el sonido. En macOS, permite **Micrófono** para NS2Bench. Si tienes dos capturadoras idénticas, NS2Bench empareja el sonido con la imagen según la posición USB.

## ¿Cuánta CPU usa la captura?

En macOS, alrededor del 12 % de un núcleo a 1080p60 (JPEG y H.264 por hardware). En Linux, las capturadoras MJPEG se transmiten sin recodificar.

# Dispositivos de entrada

## La etiqueta Bridge sigue en rojo.

- **Placa Linux:** comprueba que la placa tiene alimentación y es accesible en la dirección de Ajustes → Dispositivos, y que el archivo de token coincide con el token de la placa.
- **Pico:** comprueba el puerto serie y que la velocidad sea 921600 baudios; TX y RX deben cruzarse (TX del adaptador → GP1 de la Pico, RX del adaptador → GP0 de la Pico) y GND debe estar conectado.
- **CH9329:** usa la misma velocidad que el módulo (normalmente 9600).

Después comprueba que el dispositivo de entrada está conectado al puerto USB de la consola y que la consola está despierta.

## Las pulsaciones no se registran.

La Switch 2 necesita que una pulsación dure al menos unos 50 ms. NS2Bench mantiene cada pulsación al menos 75 ms. Si Bridge está en verde pero no pasa nada, puede que la consola esté en reposo o con la pantalla atenuada: la primera pulsación solo la despierta.

## ¿Cómo consigo C, GL y GR?

GL y GR son los dos botones traseros del Mando Pro de Nintendo Switch 2; los Joy-Con 2 no los tienen. Para usarlos (y C), usa una placa Linux y elige **La consola ve → Mando Pro** en el panel Controlando. La consola verá entonces un Mando Pro de Switch 2. Si no, C funciona como atajo de GameChat.

## ¿Puedo usar mi propio mando?

Sí. Conéctalo al ordenador; NS2Bench lo asigna por posición de botón (el botón frontal inferior es B).

## ¿Puedo escribir con un teclado?

Elige **Tu teclado envía → Teclado y ratón** en el panel Controlando. La consola ve un teclado USB real. Necesitas una placa Linux, una Pico en modo estándar o un CH9329.

## ¿Qué pasa cuando la consola entra en reposo?

Una placa alimentada por el USB de la consola se queda sin corriente y se reinicia cuando la consola despierta. NS2Bench se vuelve a conectar solo. Las pulsaciones hechas mientras Bridge está desconectado se descartan y nunca se repiten después.

## ¿Puede NS2Bench despertar la consola del reposo?

Todavía no. La Switch 2 no tiene Wake-on-LAN, y una placa alimentada por el puerto USB de la consola se apaga con ella (probado: Bridge se desconecta unos 7 segundos después del modo de reposo; al despertar la consola, la placa arranca y NS2Bench se reconecta solo, listo en unos 30 segundos). Se están explorando dos vías: alimentar la placa por separado (un divisor de alimentación USB que no devuelva corriente a la consola) para que siga despierta, o HDMI-CEC "Sincronizar con el estado de encendido del televisor", donde un dispositivo CEC en el lado HDMI despierta la consola. Mientras tanto, despiértala con su botón de encendido. Al despertar la consola, la placa arranca sola y NS2Bench se vuelve a conectar en unos 30 segundos.

# Red sin conexión para la consola

## ¿Necesito una red?

No. La captura y la entrada USB funcionan sin red. La red de la consola es solo para herramientas que hablan con la consola por LAN.

## ¿Cómo doy a la consola una LAN sin internet?

Conecta el Ethernet de la consola (puerto de la base o adaptador USB) a un adaptador Ethernet dedicado de tu Mac u ordenador Linux y ejecuta `ns2bench-netd`. Reparte direcciones y solo responde a la prueba de conexión. O usa un router, una VLAN o una red Wi-Fi con internet bloqueado. Consulta la [guía de configuración](docs/setup/README.md#optional-offline-console-network).

## ¿Puedo desactivar el bloqueo de internet?

No. No es un ajuste. NS2Bench nunca reenvía, hace NAT ni sirve de proxy para el tráfico de la consola, y nunca consulta un servidor DNS externo.

## Uso Windows.

El servicio de red funciona en macOS y Linux. En Windows, ejecútalo en una placa Linux o un PC que gestione el puerto Ethernet de la consola y contrólalo desde la pestaña Red ([asistente remoto](docs/REFERENCE.md#remote-helper-windows-hosts)), o usa un router con internet bloqueado.

# Agentes y API

## ¿Qué agentes funcionan?

Cualquier cliente MCP: Claude Code, Codex, opencode y otros. Ajustes → API y MCP muestra una configuración lista para copiar.

## ¿Puedo detener a un agente?

Sí. **Bloquear agentes (solo manual)** en el panel Controlando detiene la entrada de agentes al instante. Solo tú, desde la ventana de NS2Bench, puedes volver a desactivarlo.

## ¿Es segura la API en mi red?

El motor solo escucha en `127.0.0.1` por defecto. Escuchar en otra dirección requiere un token. Las páginas web de otros sitios no pueden enviarle comandos.

## ¿Dónde está documentada la API?

En [docs/REFERENCE.md](docs/REFERENCE.md#agents-mcp-and-http-api).

# App y actualizaciones

## ¿Cómo funcionan las actualizaciones?

La app consulta las versiones de GitHub a diario y avisa cuando hay una actualización. Cada descarga se comprueba con el SHA-256 de la versión antes de instalarse. La descarga e instalación automáticas están desactivadas por defecto; actívalas en Ajustes → Actualizaciones.

## macOS dice que la app está dañada o no se puede comprobar.

La app aún no está notarizada. Ejecuta `xattr -dr com.apple.quarantine /Applications/NS2Bench.app` una vez y luego ábrela.

## Windows dice que ha protegido mi PC.

Elige **Más información → Ejecutar de todas formas**. El instalador aún no está firmado con un certificado de pago.

## macOS vuelve a pedir Cámara o Micrófono tras una actualización.

Desde la 0.0.3, todas las versiones de NS2Bench llevan la misma firma, así que macOS pregunta una vez y lo recuerda, también tras reinicios y actualizaciones. Al actualizar desde la 0.0.2 o anterior, pregunta una última vez.

## ¿Cómo cambio el tema o el idioma?

Ajustes → Apariencia. El botón de la luna en la barra superior cambia entre claro y oscuro.

## ¿Dónde están mis clips, capturas y registros?

En `~/.ns2bench/`: clips, informes de pruebas y un archivo de registro por día en `~/.ns2bench/logs` (se guardan 7 días). Las capturas van a tu carpeta Descargas. No se sube nada a ningún sitio.

## ¿Puede NS2Bench avisarme?

Sí. **Ajustes → Notificaciones** activa notificaciones de escritorio para cuando se pierde Bridge o la imagen, terminan pruebas y clips, un agente empieza a controlar o hay una actualización lista. Por defecto solo aparecen mientras NS2Bench está en segundo plano.

## ¿Cómo instalo la herramienta de línea de comandos?

**Ajustes → Instalar → Instalar la herramienta de línea de comandos** pone `ns2bench` en tu PATH (macOS puede pedir tu contraseña una vez; el `.deb` de Linux ya la instala). Después ejecuta `ns2bench --help`. Puede hacer todo lo que hacen la API y MCP.

# Solución de problemas

## ¿Cómo informo de un error?

Haz clic en **Ayuda** en la barra superior y luego en **Guardar informe** (un zip con registros, estado y una captura; sin tokens), y adjúntalo a un [issue de GitHub](https://github.com/phantomptr/ns2bench/issues/new/choose).

## ¿Qué significan los colores del registro?

Las líneas azules son actividad normal, las ámbar son avisos que pueden resolverse solos y las rojas necesitan tu atención. Para los problemas que NS2Bench reconoce, un recuadro azul bajo la línea explica qué pasó y qué hacer. **Problemas** en el panel Registros muestra solo avisos y errores; **Todo** añade detalles como cada llamada a la API.

## ¿Qué hace doctor?

**Ayuda → Ejecutar la comprobación** en la app (o `ns2bench doctor`) comprueba el vídeo, la imagen de la consola, el OCR, Bridge, el estado USB, la calibración y el aislamiento de red, y te dice qué parte falla y qué hacer. `--probe` además pulsa RIGHT y LEFT en el menú HOME para probar la entrada de extremo a extremo.

## ¿Dónde puedo pedir ayuda?

En [Discord](https://discord.gg/fzK3xddtrM).
