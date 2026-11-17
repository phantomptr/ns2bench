# Configurando o NS2Bench

O NS2Bench permite que você, ou um agente de IA, veja, ouça e controle um Nintendo Switch 2 a partir de um computador.
Este guia mostra como montar tudo.

![Visão geral da montagem do NS2Bench](overview.svg)

Toda montagem tem duas partes independentes:

- **Olhos (sempre):** o HDMI do console vai para uma placa de captura USB. A placa entrega vídeo e áudio ao computador.
- **Mãos (opcional):** um pequeno dispositivo de entrada se conecta à porta USB do console. O computador envia comandos para ele. O console vê um controle com fio, um teclado e um mouse.

Pessoas usam a página do NS2Bench no navegador ou o aplicativo para desktop. Agentes usam a API HTTP ou o MCP no mesmo computador (`127.0.0.1:7630`). Todos compartilham uma única bancada, e você vê cada ação dos agentes ao vivo.

**O console fica offline.** O NS2Bench nunca dá acesso à internet a ele. Captura e entrada USB não precisam de rede nenhuma.

## Qual montagem eu preciso?

| O que você tem | Montagem | Conexão de entrada em Configurações |
|---|---|---|
| Só uma placa de captura | [Só visualização](#view-only) | Só visualização |
| Uma placa Linux com modo de dispositivo USB (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5) | [Placa Linux](#linux-usb-gadget-board) | Bridge de rede |
| Um Raspberry Pi Pico (RP2040) e um adaptador USB-UART de 3,3 V | [Pico](#raspberry-pi-pico) | Bridge serial |
| Um módulo CH9329 (só teclado e mouse) | [CH9329](#ch9329-module) | CH9329 |
| Você também quer o console em uma rede local (ferramentas homebrew) | [Rede offline](#optional-offline-console-network) | (aba Rede) |

Não tem certeza? Comece com **Só visualização** e adicione um dispositivo de entrada depois. As configurações de entrada mudam sem reiniciar o aplicativo.

Se você quer todos os botões do controle, incluindo C, GL e GR, use uma placa Linux. O modo **Pro** dela mostra ao console um Switch 2 Pro Controller. O Pico e o modo padrão da placa cobrem todos os botões, exceto C, GL e GR. O CH9329 não tem controle nenhum.

## Antes de começar: seu computador

Instale o aplicativo a partir da [versão mais recente](https://github.com/phantomptr/ns2bench/releases/latest) e depois:

### macOS (14 ou mais recente)

- **Nada mais para instalar.** Captura, som e leitura de texto na tela já vêm no aplicativo.
- Primeira execução: se o macOS disser que a Apple não pôde verificar o .dmg, ou que o aplicativo está danificado ou não pode ser verificado, rode `xattr -dr com.apple.quarantine ~/Downloads/NS2Bench-*.dmg` antes de abrir o .dmg (ou `xattr -dr com.apple.quarantine /Applications/NS2Bench.app` depois de instalado), ou clique em **Abrir Mesmo Assim** em Ajustes do Sistema → Privacidade e Segurança. Só na primeira instalação: atualizações de dentro do app não precisam disso.
- Permita **Câmera** (a imagem), **Microfone** (o som do console) e **Rede Local** (o dispositivo de entrada) quando for pedido. Você pode mudar isso depois em Ajustes do Sistema → Privacidade e Segurança.
- **Clipes** (Salvar clipe) precisam do ffmpeg: `brew install ffmpeg`.
- Adaptadores seriais (Pico, CH9329) funcionam sem drivers; eles aparecem como `/dev/cu.usbserial-…` ou `/dev/cu.usbmodem…`.

### Windows 10/11

- **Instale o ffmpeg** (captura, som e clipes usam ele): `winget install Gyan.FFmpeg` e depois reinicie o NS2Bench. O NS2Bench também encontra o ffmpeg instalado pelo Chocolatey ou Scoop, ou um `ffmpeg.exe` ao lado de `ns2bench-app.exe`.
- O SmartScreen pode dizer "O Windows protegeu o computador": escolha **Mais informações → Executar assim mesmo**.
- Adaptadores USB-UART podem precisar do driver do fabricante (CH340/CH341 ou CP210x). A porta aparece como `COM3` ou parecido no Gerenciador de Dispositivos.
- Opcional: [Tesseract](https://github.com/UB-Mannheim/tesseract/wiki) para leitura de texto na tela.
- O serviço de rede offline do console não roda no Windows: use um [auxiliar Linux ou um roteador](#optional-offline-console-network).

### Linux

- **ffmpeg:** o `.deb` instala para você. Com o AppImage, instale você mesmo (`sudo apt install ffmpeg`, `sudo dnf install ffmpeg`, `sudo pacman -S ffmpeg`).
- **Permissões:** adicione seu usuário aos grupos donos das placas de captura e das portas seriais, depois saia e entre de novo na sessão:
  `sudo usermod -aG video,dialout $USER` (Arch: `video,uucp`).
- AppImage: `chmod +x NS2Bench-*.AppImage`. Se ele não abrir, instale o FUSE 2 (`sudo apt install libfuse2`).
- Opcional: `sudo apt install tesseract-ocr` para leitura de texto na tela.
- As portas seriais aparecem como `/dev/ttyACM0` (Pico) ou `/dev/ttyUSB0` (a maioria dos adaptadores).

## A placa de captura (todas as montagens)

- Qualquer placa de captura HDMI USB **UVC** funciona. O objetivo é 1080p60. Testada: UGREEN 15389 (MS2130).
- Ligue a **saída HDMI** da base do console na entrada HDMI da placa. Ligue a placa ao computador por USB.
- Escolha a placa, a resolução, a taxa de quadros e a qualidade no painel **Vídeo**. Ele lista só o que a sua placa oferece.
- O **áudio** também vem da placa de captura. Não existe cabo de áudio separado.
- **macOS:** um pequeno auxiliar embutido faz a captura. Ele compartilha a placa com o OBS: se o OBS já estiver usando a placa, o NS2Bench usa o formato do OBS e nunca toma a placa dele. Na primeira execução, permita **Câmera** (imagem), **Microfone** (áudio do console) e **Rede Local** (o Bridge).
- **Linux e Windows:** o NS2Bench usa o `ffmpeg` (instale primeiro). A placa pode precisar estar livre, então feche o OBS ou outros aplicativos que a estejam usando se a imagem não aparecer.
- Rode `ns2bench devices` para listar as placas de captura que o NS2Bench enxerga.

## Só visualização

![Só visualização: uma placa de captura, sem dispositivo de entrada](view-only.svg)

A bancada mais simples. Você pode assistir ao vivo, gravar, tirar capturas de tela em resolução total e ler texto na tela (OCR). Agentes também podem assistir, mas ninguém pode pressionar botões.

**Peças**

- Console na base
- Placa de captura HDMI USB (UVC)
- Cabo HDMI e o cabo USB da placa

**Passos**

1. Ligue a saída HDMI da base à placa de captura.
2. Ligue a placa de captura ao computador.
3. Inicie o NS2Bench e abra `http://127.0.0.1:7630` (ou o aplicativo para desktop).
4. Escolha sua placa no painel **Vídeo**.
5. Em **Configurações → Dispositivos**, defina **Conexão de entrada** como **Só visualização** e clique em **Aplicar conexão de entrada**.

**Confira se funciona**

- O indicador **vídeo** na barra superior fica verde e a imagem do console aparece.
- O indicador **Bridge** mostra *não configurado*. Isso é esperado aqui.

**Problemas comuns**

- *Imagem preta:* acorde o console; confira se ele está na base e se a base está ligada na energia. Tente outro cabo HDMI.
- *Placa não aparece na lista:* no macOS, permita o acesso à Câmera. No Linux ou Windows, feche qualquer aplicativo que esteja usando a placa.
- *Sem som:* no macOS, permita o acesso ao Microfone. Confira se a placa é a fonte de áudio selecionada.

## Placa Linux USB gadget

![Placa Linux: USB para o console, Ethernet para o computador](input-linux-board.svg)

Uma pequena placa Linux se passa por dispositivos USB. Ela roda o **NS2Bench Bridge** (`ns2bench-board`). Esta é a montagem testada (Luckfox Pico Pro/Max). Ela suporta os dois modos de controle:

- **Padrão** (padrão de fábrica): o console vê um controle com fio estilo HORI + teclado USB + mouse USB.
- **Pro**: o console vê um Switch 2 Pro Controller, com todos os botões, incluindo C, GL e GR. Sem teclado nem mouse neste modo.

Troque de modo em **O console vê** no painel Controlando. Leva cerca de 8 segundos.

**Peças**

- Uma placa Linux com modo de dispositivo USB (OTG): Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W ou Pi 4/5 usando a porta USB-C
- Um cabo USB da porta **OTG** da placa até a porta USB da base
- Um cabo Ethernet até o computador (ou use Wi-Fi ou a rede USB da placa)
- A montagem da placa de captura acima

**Passos**

1. Instale o NS2Bench Bridge na placa pelo app: **Configurações → Instalar → Placa Bridge**, informe o host SSH dela e clique em **Instalar na placa**. (A partir do código-fonte: `make board-install BOARD=<ssh-host>`; adicione `BOARD_TARGET=aarch64-unknown-linux-musl` para um Raspberry Pi de 64 bits.)
2. Ligue a porta USB **OTG** da placa na porta USB da base.
3. Ligue a Ethernet da placa ao computador (ou coloque os dois na mesma rede privada).
4. Inicie o Bridge se ele ainda não estiver rodando: `ssh <ssh-host> /opt/ns2bench/ns2bench-hidd.sh start --bind <board-ip>`.
5. Confira se o arquivo de token `~/.ns2bench/hidd.token` no computador é igual ao token da placa.
6. Em **Configurações → Dispositivos**, defina:
   - **Conexão de entrada:** Bridge de rede
   - **Endereço do Bridge:** o IP da placa (padrão `192.168.79.2`)
   - **Porta do Bridge:** `7631`
   - **Arquivo de token do Bridge:** `~/.ns2bench/hidd.token`
7. Clique em **Aplicar conexão de entrada**.

**Confira se funciona**

- O indicador **Bridge** fica verde e mostra o tempo de ida e volta e *usb configured*.
- Pressione um botão no Joy-Con da tela. O console reage e o botão acende.

**Quando o console entra em repouso**

Uma placa alimentada pela USB do console perde energia quando o console entra em repouso e liga de novo quando ele acorda. Deixe o NS2Bench aberto: ele percebe em cerca de três segundos e se reconecta sozinho. A entrada volta a funcionar assim que a placa termina de iniciar e o console configura a USB dele.

Para isso funcionar, a placa precisa se configurar sozinha ao ligar. Adicione um script de inicialização na sua placa que rode `/opt/ns2bench/gadget.sh` e depois `/opt/ns2bench/ns2bench-hidd.sh start`. Teste um repouso e um despertar reais com a sua placa e a sua base.

**Problemas comuns**

- *Bridge offline · reconectando:* confira o cabo e o IP da placa. Faça ping na placa. Confira se o Bridge está rodando nela.
- *Bridge conectado, mas a usb não está configurada:* acorde o console. Confira se o cabo vai na porta **OTG** da placa, e não em uma porta só de energia. Alguns cabos só transmitem energia.
- *O mouse não faz nada no menu HOME:* normal. O console lê o mouse, mas só alguns softwares mostram um cursor.
- *Teclado ignorado:* o suporte a teclado USB do console fica em Configurações do console → Controles e acessórios.
- Na placa, pare e inicie as coisas só com `ns2bench-hidd.sh` e `gadget.sh`. Não encerre processos por padrão de nome.

## Raspberry Pi Pico

![Raspberry Pi Pico: USB para o console, UART para um adaptador USB no computador](input-pico.svg)

Um Raspberry Pi Pico (RP2040) rodando o firmware do NS2Bench Bridge. A porta USB dele vai para o console e mostra o mesmo controle + teclado + mouse que uma placa Linux no modo padrão. O computador conversa com ele por um link serial (UART) através de um adaptador USB-UART separado.

> O firmware do Pico já é compilado, mas ainda não foi testado em hardware real. Relatos são bem-vindos.

**Peças**

- Raspberry Pi Pico (RP2040) e um cabo micro-USB
- Um adaptador USB-UART de **3,3 V** (CP2102, FT232 ou CH340)
- Três fios jumper
- A montagem da placa de captura acima

**Passos**

1. Grave o Pico pelo app: **Configurações → Instalar → Gravar no Pico**, depois segure o botão **BOOTSEL** enquanto liga o Pico no **computador**. O app copia o firmware para a unidade **RPI-RP2** e o Pico reinicia. Desconecte-o. (A partir do código-fonte: `make pico` gera `firmware/pico/ns2bench-pico.uf2`; copie-o você mesmo para a unidade RPI-RP2.)
2. Ligue os fios do Pico ao adaptador:
   - Pico **GP0** (pino 1, UART0 TX) → adaptador **RX**
   - Pico **GP1** (pino 2, UART0 RX) ← adaptador **TX**
   - Pico **GND** (pino 3) — adaptador **GND**
   - Deixe o pino de 3,3 V / 5 V do adaptador desconectado. O Pico recebe energia do console.
3. Ligue o micro-USB do Pico na porta USB da base.
4. Ligue o adaptador no computador.
5. Em **Configurações → Dispositivos**, defina:
   - **Conexão de entrada:** Bridge serial
   - **Porta serial:** o adaptador (por exemplo `/dev/cu.usbserial-XXXX` no macOS, `/dev/ttyUSB0` no Linux, `COM3` no Windows)
   - **Taxa de baud:** `921600`
6. Clique em **Aplicar conexão de entrada**.

Opcional: compile com `NS2BENCH_TOKEN=... make pico` para que o Pico exija um token (configure o mesmo arquivo de token no computador). Compile com `NS2BENCH_BAUD=115200` se o seu adaptador não suportar 921600.

**Confira se funciona**

- O indicador **Bridge** fica verde e mostra *usb configured*.
- Um toque no Joy-Con da tela chega ao console.

**Problemas comuns**

- *Nenhuma porta serial aparece:* instale o driver do adaptador (CP210x, FTDI ou CH340) e depois reconecte-o.
- *Bridge offline:* TX e RX provavelmente estão trocados. TX sempre vai em RX. Confira também o GND.
- *Respostas embaralhadas ou nenhuma resposta:* a taxa de baud precisa ser igual à do firmware (921600, a menos que você tenha compilado com outro valor).
- *Nunca use um adaptador de 5 V.* Os pinos do Pico são só de 3,3 V.

## Módulo CH9329

![CH9329: USB para o console, UART para o computador](input-ch9329.svg)

Um módulo CH9329 transforma comandos seriais em um teclado e mouse USB. Ele é barato e simples, mas **não tem controle**: nada de botões, nada de analógicos. Use-o só para digitar e apontar.

**Peças**

- Um módulo CH9329
- Um adaptador USB-UART de 3,3 V, a menos que o cabo do seu módulo já tenha USB-serial embutido
- A montagem da placa de captura acima

**Passos**

1. Ligue o lado USB (HID) do módulo na porta USB da base.
2. Ligue o lado UART ao adaptador: TX do módulo → RX do adaptador, RX do módulo ← TX do adaptador, GND — GND. (Pule isto se o módulo tiver USB-serial embutido.)
3. Ligue o adaptador (ou o cabo serial do módulo) no computador.
4. Em **Configurações → Dispositivos**, defina:
   - **Conexão de entrada:** CH9329
   - **Porta serial:** a porta do adaptador
   - **Taxa de baud:** a configuração do módulo, geralmente `9600`
5. Clique em **Aplicar conexão de entrada**.

**Confira se funciona**

- O indicador **Bridge** fica verde.
- Digitar no modo de teclado nativo aparece no console (por exemplo, em um campo de texto).

**Problemas comuns**

- *Os botões do controle não fazem nada:* esperado. O CH9329 é só teclado e mouse.
- *Nada acontece:* confira a taxa de baud e troque TX/RX se precisar.
- *Teclado ignorado pelo console:* confira Configurações do console → Controles e acessórios → Teclado USB.

## Opcional: rede offline do console

![Três formas de dar ao console uma rede offline](network.svg)

Você só precisa disto para ferramentas de rede local, como carregadores homebrew. Captura e entrada nunca precisam disso. As variantes A–C usam uma rede separada para o console. A variante D mantém o console no Wi-Fi da sua casa.

Seja qual for a variante escolhida, o console fica **sem internet**. O NS2Bench nunca encaminha tráfego, nunca ativa compartilhamento de internet, NAT ou proxy, e nunca consulta um servidor DNS externo. O DNS local dele responde só ao teste de conectividade do console; qualquer outro nome é recusado. Toda consulta que o console tenta aparece no log de **rede**.

O link do Bridge entre o computador e uma placa Linux é separado. Ele pode usar a sua LAN normal.

### A · Cabo direto (macOS ou Linux)

**Peças:** um adaptador Ethernet extra no computador, só para o console, um cabo Ethernet e a porta LAN da base (ou um adaptador USB-Ethernet no console).

1. Dê ao adaptador dedicado um endereço privado fixo, por exemplo `192.168.80.1/24`. O NS2Bench não configura a interface para você.
2. Descreva essa interface no `bench.json` (use o nome de interface do seu adaptador):
   ```json
   "network": { "interface": "en7", "address": "192.168.80.1", "subnet": "192.168.80.0/24",
                "pool_start": "192.168.80.10", "pool_end": "192.168.80.13" }
   ```
   Compile o serviço de rede: `make netd`.
3. Verifique a configuração sem abrir nenhuma porta: `native/target/release/ns2bench-netd --check bench.json`.
4. Instale-o como serviço:
   - **macOS:** copie o binário para `~/.ns2bench/bin/ns2bench-netd`, preencha `launchd/ns2bench-netd.plist.template` e copie-o para `~/Library/LaunchAgents/`. Não precisa de root.
   - **Linux:** instale-o em `/opt/ns2bench/ns2bench-netd` e use a unidade systemd `launchd/ns2bench-netd.service`.
5. Ligue a Ethernet do console ao adaptador dedicado.
6. Em **Configurações → Rede**, escolha **Direto e isolado: computador ↔ console**. Ative o DHCP local, o DNS e a página de teste de conectividade conforme precisar e depois aplique.

O console recebe um endereço do pool (por exemplo `192.168.80.10`–`192.168.80.13`). A aba **Rede** mostra o status ao vivo e as concessões.

O serviço se recusa a iniciar se o encaminhamento de IP estiver ativado, ou se não conseguir verificar isso. Isso é proposital.

### B · Roteador isolado, VLAN ou Wi-Fi (qualquer sistema)

1. Configure um roteador, VLAN ou rede Wi-Fi para o console com a **internet bloqueada, incluindo IPv6**. Uma LAN doméstica comum não é isolada.
2. Deixe o roteador distribuir os endereços (DHCP).
3. Conecte o console a ele por Ethernet ou Wi-Fi.
4. Em **Configurações → Rede**, escolha **Roteador / VLAN / Wi-Fi isolado**. Isso desativa o DHCP e o DNS locais do NS2Bench para não entrarem em conflito com o roteador.

### C · Computador Windows com um auxiliar remoto

O Windows não consegue rodar o serviço de rede localmente. Rode-o em uma placa ou PC Linux e controle-o pelo Windows.

1. No auxiliar Linux, configure o `ns2bench-netd` como na variante A. O auxiliar fica com a Ethernet do console.
2. Crie um token: `openssl rand -hex 16 > ~/.ns2bench/netd-control.token`. Copie o mesmo arquivo para o computador Windows.
3. Na configuração do auxiliar, adicione um listener de gerenciamento em um endereço privado que **não** esteja na rede do console:
   ```json
   "network": { "control": { "listen": "192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
4. Na configuração do NS2Bench no Windows, aponte para o auxiliar:
   ```json
   "network": { "helper": { "url": "http://192.168.79.1:7632", "token_file": "~/.ns2bench/netd-control.token" } }
   ```
5. Mantenha o link de gerenciamento privado: um cabo direto ou a rede USB da placa. O tráfego dele é HTTP simples.

A aba **Rede** no Windows passa a mostrar o status e os controles do auxiliar. Ou dispense o auxiliar e use a variante B.

### D · Rede doméstica compartilhada (Wi-Fi, sem hardware extra)

![Consoles no Wi-Fi de casa sem internet: o gateway deles é o próprio endereço, e o computador responde só ao teste de conexão deles](shared-network.svg)

Os consoles ficam no Wi-Fi normal da sua casa, junto com o seu computador. Nada mais para comprar ou ligar. Cada console usa **configurações manuais** que não deixam nenhum lugar para onde mandar tráfego, e o NS2Bench no seu computador responde só ao teste de conexão dele, então o console continua dizendo que está conectado.

**Como a internet é bloqueada**

| Caminho de saída | O que o bloqueia |
|---|---|
| Consultar um nome da Nintendo (eShop, atualizações, contas, telemetria) | O DNS do NS2Bench recusa todos os nomes, exceto o teste de conexão. Cada tentativa aparece no vigia de vazamentos e no log de **rede**. |
| Conectar a um servidor pelo endereço IP | O gateway do console é **o próprio endereço dele**, então nada do que ele envia sai da sua rede doméstica, seja qual for o endereço. |
| IPv6 | Seu roteador não pode distribuir IPv6, ou desligue o IPv6. |
| Seu computador repassar o tráfego | O console nunca manda tráfego para o seu computador como gateway, e o auxiliar para se o encaminhamento ou o compartilhamento de internet for ligado. |
| O console voltar às configurações automáticas (uma redefinição, uma atualização, alguém mudar as configurações) | O **vigia de vazamentos** avisa você em cerca de 10 minutos. Adicione também uma pausa no roteador (passo 7). |

A última linha é a única coisa que uma rede compartilhada não consegue descartar: ali, a trava são as próprias configurações do console. Se você precisa de "offline mesmo depois de restaurar as configurações de fábrica", use a variante A ou B, em que não existe nenhum caminho para a internet.

**Você precisa de:** NS2Bench no macOS ou Linux com o auxiliar de rede instalado, seu computador e os consoles na mesma rede doméstica, e um roteador que não distribua IPv6 (ou com o IPv6 desligado).

1. **Reserve endereços no seu roteador** (Google Home, o app do roteador ou a página web dele): um para o seu computador e um para cada console. Anote-os, por exemplo computador `192.168.86.199`, consoles `192.168.86.251` e `192.168.86.252`.
2. **Instale o auxiliar de rede**, se ainda não instalou: **Configurações → Instalar → Auxiliar de rede**.
3. Abra **Configurações → Rede** e escolha **Rede doméstica compartilhada**.
4. **Escolha o adaptador de rede** que está na rede dos consoles. A lista mostra cada adaptador com o endereço, por exemplo `en10 · Belkin USB-C LAN · 192.168.86.199/24`.
5. Digite os **endereços dos consoles**, marque as três confirmações (sem IPv6, endereço do computador reservado, nenhum outro Wi-Fi salvo nos consoles) e clique em **Salvar e usar esta rede**. O NS2Bench guarda um backup do seu arquivo de configurações. Depois clique em **Reiniciar auxiliar de rede**.
6. **Em cada console:** Configurações do sistema → Internet → seu Wi-Fi → **Alterar configurações**:
   - Configurações de endereço IP: **Manual**. Endereço IP: o endereço reservado do console. Máscara de sub-rede: `255.255.255.0`. **Gateway: o próprio endereço do console.**
   - Configurações de DNS: **Manual**. DNS primário e secundário: **o endereço do seu computador**.
   - Salve e rode o teste de conexão. Ele passa, e o console diz que está conectado.
   O NS2Bench mostra esses valores exatos para cada console em **Digite isto em cada console**.
7. **Segunda trava recomendada:** no app do seu roteador, pause ou bloqueie a internet para os consoles (Google Home: Wi-Fi → Family Wi-Fi → pausar). Confira se o teste de conexão continua passando com a pausa ligada.
8. Apague todas as outras redes Wi-Fi salvas em cada console e evite jogar no modo sem fio local com um console de firmware mais novo (os consoles podem passar atualizações do sistema uns para os outros desse jeito).

**Como ler o vigia de vazamentos** (Configurações → Rede, verificado a cada 30 segundos):

- **Protegido:** o console consultou o NS2Bench há pouco. Mostra quando, o último teste de conexão e quantos nomes foram recusados.
- **Na sua rede mas sem usar o NS2Bench:** ele não consulta há 10 minutos, mesmo estando na rede. Pode ter voltado às configurações automáticas e estar online. Confira as configurações dele. O NS2Bench também envia uma notificação na área de trabalho.
- **Fora da rede no momento:** desligado, em repouso ou longe. Nada a fazer.

**Problemas comuns**

- *O console não conecta:* confira se o DNS dele é o endereço do seu computador, se o auxiliar está rodando (Configurações → Instalar) e se o adaptador escolhido no passo 4 está na rede dos consoles. Só os endereços da lista recebem respostas, então confira também o IP do console.
- *O console recusa o próprio endereço como gateway:* use no lugar um endereço livre fora da faixa DHCP do roteador, por exemplo `192.168.86.254`. Garanta que nada use esse endereço, nem agora nem depois.
- *O endereço do seu computador mudou:* os consoles não conseguem conectar (ficam offline). Reserve o endereço no roteador e faça a configuração de novo.
- *Só uma rede por vez:* o auxiliar atende ou esta rede compartilhada ou um cabo direto (variante A), não as duas.

**Problemas comuns**

- *A aba Rede diz indisponível:* o serviço não está rodando, ou ele e a interface usam pastas de estado diferentes. No Windows, confira `network.helper` e se os dois arquivos de token são iguais.
- *O console não recebe endereço:* confira o nome da interface e a sub-rede na configuração, e se o cabo vai no adaptador dedicado.
- *O console diz que não tem internet:* está correto. O teste de conectividade passa localmente, mas nada vai além disso.

## Agentes

Quando os indicadores estiverem verdes, os agentes podem usar a bancada. Abra **Configurações → API e MCP** para ver o endereço da API, exemplos de requisição e uma configuração de MCP pronta para copiar para Claude Code, Codex ou opencode. Agentes e pessoas compartilham os mesmos controles, e a chave de modo somente manual bloqueia a entrada dos agentes a qualquer momento.

## Observações sobre o Windows

- A captura usa o `ffmpeg` com DirectShow. Feche outros aplicativos que estejam usando a placa.
- A placa Linux, o Pico e o CH9329 funcionam todos no Windows. As portas seriais se chamam `COM3`, `COM4` e assim por diante.
- Não há serviço de rede local no Windows. Use um auxiliar remoto (variante C) ou um roteador isolado (variante B).
