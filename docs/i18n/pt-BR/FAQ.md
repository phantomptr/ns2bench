# Geral

## O que é o NS2Bench?

Uma bancada para um Nintendo Switch 2 na sua mesa. Ele mostra a imagem e o som do console no seu computador e deixa você, ou um agente de IA, apertar os botões. Você pode gravar clipes, ler texto na tela e rodar testes repetíveis.

## Para quem é?

Desenvolvedores testando software em hardware real, quem quer automatizar verificações repetitivas, experimentos de acessibilidade e qualquer pessoa que queira que um agente de IA opere um console enquanto assiste.

## Ele modifica meu console?

Não. O NS2Bench só faz o que uma TV e um controle com fio fazem: recebe HDMI e aperta botões via USB. Nada é instalado no console. Funciona com qualquer Switch 2 do jeito que sai da caixa, em qualquer versão do sistema: não é preciso jailbreak, modificações nem homebrew. Um console com jailbreak também funciona.

## O console fica online?

Não. O NS2Bench nunca dá acesso à internet ao console. Captura e entrada USB não precisam de rede nenhuma. A rede opcional do console só responde ao teste de conexão dele e nunca encaminha tráfego. Veja [Rede offline do console](#offline-console-network).

## É gratuito?

Sim. O NS2Bench é gratuito e de código aberto, sob a licença GPL-3.0. Você pode usar, modificar e compartilhar. Ao repassá-lo, modificado ou não, mantenha o crédito "NS2Bench, created by PhantomPtr" e a licença; ao repassar uma versão modificada, ofereça o código-fonte a quem recebe e indique que foi modificada (veja NOTICE). Se for útil, você pode [apoiá-lo](README.md#support).

## Tem relação com a Nintendo?

Não. O NS2Bench é um projeto independente. Nintendo Switch é uma marca registrada da Nintendo.

# Configuração

## Do que eu preciso?

Uma placa de captura para vídeo e som (sempre) e um dispositivo de entrada se quiser apertar botões:

- uma placa Linux com porta USB de dispositivo (Luckfox Pico Pro/Max, Raspberry Pi Zero 2 W, Pi 4/5),
- ou um Raspberry Pi Pico com um adaptador USB-UART de 3,3 V,
- ou um módulo CH9329 (só teclado e mouse).

O [guia de configuração](docs/setup/README.md) tem um diagrama e os passos para cada um.

## O que preciso instalar no computador?

- **macOS:** nada; só permita Câmera, Microfone e Rede Local. `brew install ffmpeg` se quiser salvar clipes.
- **Windows:** ffmpeg (`winget install Gyan.FFmpeg`) e um driver CH340/CP210x se o seu adaptador serial precisar.
- **Linux:** ffmpeg (o `.deb` instala) e os grupos `video` e `dialout` (`sudo usermod -aG video,dialout $USER`).

Veja [Antes de começar](docs/setup/README.md#before-you-start-your-computer).

## Qual dispositivo de entrada devo escolher?

Se você quer todos os botões, incluindo C, GL e GR, use uma placa Linux: o modo Pro mostra ao console um Pro Controller do Switch 2 de verdade. Um Pico é a opção de controle mais barata (todos os botões exceto C, GL e GR). Um CH9329 dá só teclado e mouse. Você pode começar só visualizando e adicionar um dispositivo de entrada depois.

## Preciso do dock?

O console precisa de um jeito de enviar HDMI, o que normalmente é o dock. O dispositivo de entrada vai numa das portas USB do dock.

## Posso usar sem dispositivo de entrada?

Sim. Escolha **Só visualização (sem dispositivo de entrada)** em Configurações → Dispositivos. Você pode assistir, ouvir, tirar capturas de tela, salvar clipes e ler texto na tela.

## Onde configuro a placa de captura?

No painel **Vídeo**, à direita da janela principal. Ele lista só os formatos que a sua placa oferece.

## Onde configuro o dispositivo de entrada?

Em **Configurações → Dispositivos**. As mudanças valem sem reiniciar; a conexão antiga solta todos os botões primeiro.

## O app pode instalar o Bridge, o firmware do Pico ou o auxiliar de rede?

Sim, em **Configurações → Instalar**. **Gravar no Pico** coloca o firmware do Bridge num Raspberry Pi Pico (segure BOOTSEL enquanto o liga). **Instalar na placa** instala ou atualiza o Bridge numa placa Linux via SSH (configure antes o login por chave). **Instalar auxiliar** roda o auxiliar de rede offline como serviço no macOS ou Linux. Os downloads vêm da versão correspondente ao app e são verificados antes do uso.

## Como sei que está funcionando?

O indicador **vídeo** na barra superior fica verde com a taxa de quadros, e o indicador **Bridge** fica verde quando o console aceita o dispositivo de entrada. **Diagnóstico** na barra superior copia um resumo completo do status.

# Captura e som

## Quais placas de captura funcionam?

Qualquer placa de captura HDMI USB (UVC). O objetivo é 1080p60. Testada: UGREEN 15389 (MS2130), em 1080p60 e 1440p30.

## Posso usar o OBS ao mesmo tempo?

No macOS, sim: o NS2Bench compartilha a placa com o OBS e nunca a toma para si. No Windows e no Linux, o ffmpeg geralmente precisa da placa só para ele, então feche outros apps que a estejam usando.

## Não aparece imagem.

Verifique se o console está no dock e ligado, se o cabo HDMI está na **entrada** da placa e se a placa certa está selecionada no painel Vídeo. No macOS, permita **Câmera** para o NS2Bench em Ajustes do Sistema → Privacidade e Segurança. Rode o **Diagnóstico** para mais detalhes.

## Não tem som.

Clique em **Som desligado** na barra superior para ligar o som. No macOS, permita **Microfone** para o NS2Bench. Se você tiver duas placas de captura iguais, o NS2Bench junta o som com a imagem pela posição USB.

## Quanto de CPU a captura usa?

No macOS, cerca de 12% de um núcleo em 1080p60 (JPEG e H.264 por hardware). No Linux, placas MJPEG são repassadas sem recodificar.

# Dispositivos de entrada

## O indicador Bridge continua vermelho.

- **Placa Linux:** verifique se a placa está ligada e acessível no endereço em Configurações → Dispositivos, e se o arquivo de token bate com o token da placa.
- **Pico:** verifique a porta serial e se a taxa é 921600 baud; TX e RX devem ser cruzados (TX do adaptador → GP1 do Pico, RX do adaptador → GP0 do Pico) e o GND deve estar conectado.
- **CH9329:** use a mesma taxa do módulo (normalmente 9600).

Depois, verifique se o dispositivo de entrada está na porta USB do console e se o console está acordado.

## Os toques não são registrados.

O Switch 2 precisa que um toque dure pelo menos uns 50 ms. O NS2Bench segura cada toque por pelo menos 75 ms. Se o Bridge está verde mas nada acontece, o console pode estar em repouso ou com a tela escurecida: o primeiro toque só o acorda.

## Como uso C, GL e GR?

GL e GR são os dois botões traseiros do Pro Controller do Nintendo Switch 2; os Joy-Con 2 não têm. Para usá-los (e o C), use uma placa Linux e escolha **O console vê → Pro Controller** no painel Controlando. O console passa a ver um Pro Controller do Switch 2. Caso contrário, o C funciona como atalho do GameChat.

## Posso usar meu próprio controle?

Sim. Conecte-o ao computador; o NS2Bench mapeia pela posição do botão (o botão frontal de baixo é o B).

## Posso digitar com um teclado?

Escolha **Seu teclado envia → Teclado e mouse** no painel Controlando. O console vê um teclado USB de verdade. Isso exige uma placa Linux, um Pico no modo padrão ou um CH9329.

## O que acontece quando o console entra em repouso?

Uma placa alimentada pela USB do console perde energia e reinicia quando o console acorda. O NS2Bench reconecta sozinho. Toques feitos enquanto o Bridge está offline são descartados, nunca repetidos depois.

## O NS2Bench consegue tirar o console do repouso?

Sim, com uma ajudinha. Uma tela escurecida acorda com qualquer toque, e o NS2Bench faz isso sozinho. Modo de Repouso, travamento ou console desligado precisam de uma destas opções, configuradas em **Configurações → Dispositivos → Energia e despertar**:

- **HDMI-CEC (nada mecânico):** um adaptador USB-CEC (por exemplo, da Pulse-Eight) no cabo HDMI entre a base e a placa de captura envia o "ligar" que uma TV enviaria. Ative no console **Configurações do console → Saída de TV → Sincronizar com o estado de energia da TV** e instale o libcec no computador (`brew install libcec` no Mac). Ele também pode colocar o console em repouso.
- **Um comando de botão de energia:** para um apertador de botão como o SwitchBot, um webhook do Home Assistant ou um relé. É a única forma de voltar de um travamento ou do console desligado.

Depois, **Acordar o console** (na tela sem imagem, na busca, na API `POST /api/power`, na ferramenta MCP `power` e em `ns2bench power wake`) tenta o CEC, depois o botão de energia se você permitir, e depois um toque. O Switch 2 não tem Wake-on-LAN, e uma placa alimentada pela porta USB do console desliga uns 7 segundos após o Modo de Repouso; ela inicia de novo quando o console acorda, e o NS2Bench reconecta sozinho em cerca de 30 segundos.

# Rede offline do console

## Preciso de uma rede?

Não. Captura e entrada USB funcionam sem rede. A rede do console serve só para ferramentas que falam com o console por LAN.

## Como dou ao console uma LAN sem internet?

Conecte o Ethernet do console (porta do dock ou adaptador USB) a um adaptador Ethernet dedicado no seu Mac ou computador Linux e rode `ns2bench-netd`. Ele distribui endereços e só responde ao teste de conexão. Ou use um roteador, VLAN ou rede Wi-Fi com a internet bloqueada. Veja o [guia de configuração](docs/setup/README.md#optional-offline-console-network).

## Posso desligar o bloqueio de internet?

Não. Isso não é uma configuração. O NS2Bench nunca encaminha, faz NAT ou proxy do tráfego do console e nunca consulta um servidor DNS externo.

## Uso Windows.

O serviço de rede roda no macOS e no Linux. No Windows, rode-o numa placa Linux ou PC que cuide da porta Ethernet do console e controle-o pela aba Rede ([auxiliar remoto](docs/REFERENCE.md#remote-helper-windows-hosts)), ou use um roteador com a internet bloqueada.

# Agentes e API

## Quais agentes funcionam?

Qualquer cliente MCP: Claude Code, Codex, opencode e outros. Configurações → API e MCP mostra uma configuração pronta para copiar.

## Posso parar um agente?

Sim. **Bloquear agentes (só manual)** no painel Controlando para a entrada dos agentes na hora. Só você, pela janela do NS2Bench, pode desligar isso de novo.

## A API é segura na minha rede?

O motor escuta só em `127.0.0.1` por padrão. Escutar em outro endereço exige um token. Páginas web de outros sites não conseguem enviar comandos a ele.

## Onde a API está documentada?

Em [docs/REFERENCE.md](docs/REFERENCE.md#agents-mcp-and-http-api).

# App e atualizações

## Como funcionam as atualizações?

O app verifica as versões no GitHub diariamente e avisa quando há uma atualização. Cada download é conferido com o SHA-256 da versão antes de instalar. Download e instalação automáticos vêm desligados; ligue-os em Configurações → Atualizações.

## O macOS diz que o app está danificado ou não pode ser verificado.

O app ainda não é notarizado. Rode `xattr -dr com.apple.quarantine /Applications/NS2Bench.app` uma vez e depois abra-o.

## O Windows diz que protegeu meu PC.

Escolha **Mais informações → Executar assim mesmo**. O instalador ainda não é assinado com um certificado pago.

## O macOS pede Câmera ou Microfone de novo depois de uma atualização.

A partir da 0.0.3, toda versão do NS2Bench tem a mesma assinatura, então o macOS pergunta uma vez e lembra, mesmo após reiniciar ou atualizar. Ao atualizar da 0.0.2 ou anterior, ele pergunta uma última vez.

## Como mudo o tema ou o idioma?

Configurações → Aparência. O botão de lua na barra superior alterna entre claro e escuro.

## Onde ficam meus clipes, capturas de tela e logs?

Em `~/.ns2bench/`: clipes, relatórios de teste e um arquivo de log por dia em `~/.ns2bench/logs` (guardados por 7 dias). As capturas de tela vão para a sua pasta Downloads. Nada é enviado para lugar nenhum.

## O NS2Bench pode me avisar?

Sim. **Configurações → Notificações** ativa notificações na área de trabalho para quando o Bridge ou a imagem cai, testes e clipes terminam, um agente começa a controlar ou uma atualização está pronta. Por padrão, elas só aparecem enquanto o NS2Bench está em segundo plano.

## Como instalo a ferramenta de linha de comando?

**Configurações → Instalar → Instalar ferramenta de linha de comando** coloca `ns2bench` no seu PATH (o macOS pode pedir sua senha uma vez; o `.deb` do Linux já a instala). Depois rode `ns2bench --help`. Ela faz tudo o que a API e o MCP fazem.

# Solução de problemas

## Como relato um bug?

Clique em **Ajuda** na barra superior, depois em **Salvar relatório** (um zip com logs, status e uma captura de tela; sem tokens), e anexe-o a uma [issue no GitHub](https://github.com/phantomptr/ns2bench/issues/new/choose).

## O que significam as cores do log?

Linhas azuis são atividade normal, linhas âmbar são avisos que podem se resolver sozinhos e linhas vermelhas precisam de você. Para problemas que o NS2Bench reconhece, uma caixa azul abaixo da linha diz o que aconteceu e o que fazer. **Problemas** no painel Logs mostra só avisos e erros; **Tudo** acrescenta detalhes como cada chamada à API.

## O que o doctor faz?

**Ajuda → Executar a verificação** no app (ou `ns2bench doctor`) verifica o vídeo, a imagem do console, o OCR, o Bridge, o estado USB, a calibração e o isolamento de rede, e diz qual parte está com problema e o que fazer. `--probe` também aperta RIGHT e LEFT no menu HOME para testar a entrada de ponta a ponta.

## Onde posso pedir ajuda?

No [Discord](https://discord.gg/fzK3xddtrM).
