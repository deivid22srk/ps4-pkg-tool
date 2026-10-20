# PS4 PKG Tool

Aplicativo Android para **extrair arquivos `.pkg` do PS4 para uma pasta** e
**montar um `.pkg` a partir de uma pasta**, usando apenas o Storage Access
Framework (SAF). O processamento acontece no próprio aparelho, sem enviar
arquivos para servidores.

> Projeto independente, sem vínculo com a Sony Interactive Entertainment.
> As chaves usadas são keysets públicos documentados na comunidade (psdevwiki,
> shadPS4); nenhum material confidencial da Sony é incluído.

## Recursos

- Extração de PKG para uma pasta escolhida pelo usuário (SAF).
- **Extração de jogos comerciais (PKG de varejo)**: as chaves PFS são derivadas
  do próprio PKG (entradas 0x10/0x20, mesma cadeia do shadPS4) e a imagem é
  descriptografada em XTS-AES-128 durante a leitura.
- Empacotamento de uma pasta em um PKG no formato *homebrew*/não assinado.
- Análise do PKG sem copiá-lo: content ID, título, título ID, categoria,
  tamanho, quantidade de arquivos e aviso de criptografia.
- Operações longas em **Foreground Service** (`dataSync`) com notificação de
  progresso determinado, arquivo atual, velocidade, ETA e ação **Cancelar**.
- Progresso sincronizado com a interface por `StateFlow`; a tela continua
  mostrando o andamento após girar o aparelho ou ir para segundo plano.
- Interface em Jetpack Compose com Material 3 Expressive, Dynamic Color,
  temas claro/escuro, layout responsivo, edge-to-edge, ícone adaptativo e
  suporte a português (padrão) e inglês.
- Histórico local das últimas operações.
- Tratamento de erros com mensagens claras: PKG inválido/corrompido, PKG
  criptografado, espaço insuficiente, permissões, cancelamento e falhas de E/S.
- Limpeza de arquivos temporários e de saídas parciais em caso de falha.

## Limitações importantes

1. **PKGs comerciais são suportados via derivação de chaves do próprio arquivo**
   (entradas `ENTRY_KEYS`/`IMAGE_KEY`, algoritmo idêntico ao do emulador
   shadPS4). PKGs que não trazem essas entradas (ou cujos blobs não decifram
   com os keysets públicos) continuam sem suporte.
2. **O PKG criado é não assinado e não criptografado.** Ele serve como pacote
   *homebrew* para estudo e testes. Não é um PKG assinado pela Sony e a
   instalação depende do ambiente utilizado no console; não há garantia de
   instalação em firmware de fábrica.
3. O app não baixa, distribui nem contém jogos. Use apenas arquivos dos quais
   você tenha direito.
4. Arquivos maiores que aproximadamente **4 GB** são rejeitados para evitar
   estouro de `mmap`/espaço de endereçamento, especialmente em aparelhos
   32-bit. Prefira um dispositivo 64-bit (arm64-v8a/x86_64).
5. A extração e o empacotamento usam cópias temporárias no cache do app porque
   o SAF não expõe um caminho de arquivo comum. Reserve espaço livre para o
   arquivo de entrada, a saída temporária e o destino final.
6. Em Android 15+, serviços em primeiro plano do tipo `dataSync` têm limite de
   execução. Operações muito longas podem ser interrompidas pelo sistema; o app
   informa o erro e limpa os temporários na próxima execução.
7. O formato PKG/PFS é complexo e este projeto foi escrito a partir de
   documentação pública. Teste sempre em um ambiente controlado.

## Requisitos

- Android 8.0+ (API 26); o fluxo foi desenhado para Android 10+ com scoped
  storage e usa somente SAF.
- Espaço livre suficiente no cache do app e no destino escolhido.
- Um arquivo `.pkg` do qual você tenha direito, ou uma pasta para empacotar.

## Como usar

### Extrair um PKG

1. Abra o app e toque em **Selecionar PKG**.
2. Escolha o arquivo `.pkg`. O app mostra as informações e um aviso caso o PKG
   seja criptografado.
3. Toque em **Selecionar pasta de destino** e escolha uma pasta via SAF.
4. Toque em **Extrair**. A operação continua em primeiro plano; acompanhe pela
   notificação ou pela tela.
5. Ao final, use **Abrir pasta** para ir ao destino. Em caso de erro, a saída
   parcial é removida (quando possível) e uma mensagem explica o problema.

### Empacotar uma pasta

1. Toque em **Selecionar pasta** e escolha a pasta que será a raiz do PKG.
2. Opcionalmente informe **Content ID** e **Título** nos campos avançados. Se
   não forem informados, o app lê `sce_sys/param.sfo` ou gera valores
   determinísticos.
3. Toque em **Criar PKG** e escolha o arquivo de saída via SAF.
4. Acompanhe o progresso. O resultado é um PKG **não assinado**, no formato
   homebrew descrito acima.

A pasta pode conter `sce_sys/param.sfo`, `sce_sys/icon0.png`, `sce_sys/pic0.png`
e outros arquivos de metadados. O app normaliza o `param.sfo` e inclui as
entradas de metadados no PKG.

## Como funciona

- **Kotlin/Compose** cuida da interface, do SAF (`ACTION_OPEN_DOCUMENT`,
  `ACTION_OPEN_DOCUMENT_TREE`, `ACTION_CREATE_DOCUMENT`, `DocumentFile`) e do
  Foreground Service.
- **Rust (`pkg_core`)** faz o parsing do PKG, a derivação de chaves PFS
  (entradas 0x10/0x20 → RSA → SHA-256 → AES-CBC → RSA → `ekpfs`), a leitura da
  imagem PFS (com descriptografia XTS-AES-128 via [`orbis-pfs`](https://crates.io/crates/orbis-pfs)),
  a extração e a criação do PKG/PFS. A comunicação com o Kotlin é feita por JNI;
  o progresso e o cancelamento usam callback e um `AtomicBool` global.
- A leitura de PFS utiliza o crate [`orbis-pfs`](https://crates.io/crates/orbis-pfs)
  (MIT OR Apache-2.0). A escrita de PFS/PKG foi implementada neste projeto.
- O fluxo de extração copia o PKG para o cache, abre a imagem PFS, extrai para
  um diretório temporário e copia o resultado para a árvore SAF escolhida. O
  fluxo de empacotamento copia a árvore SAF para o cache, monta o PKG em um
  arquivo temporário e copia para o documento de destino.

## Compilação

O projeto é compilado exclusivamente pelo GitHub Actions. O workflow
`.github/workflows/build.yml`:

- instala JDK 17 (Temurin), Android SDK 36 e NDK `27.2.12479018`;
- instala os alvos Rust `aarch64-linux-android`, `armv7-linux-androideabi` e
  `x86_64-linux-android`;
- instala `cargo-ndk`, executa os testes Rust de host e compila as bibliotecas
  JNI para as três ABIs;
- compila `:app:assembleRelease`, assina com os secrets quando disponíveis (ou
  com a chave de debug como fallback) e publica o APK como artifact;
- em tags `v*`, cria uma GitHub Release com o APK.

### Assinatura de release

Configure estes secrets no repositório para assinar com sua própria chave:

| Secret | Conteúdo |
| --- | --- |
| `KEYSTORE_BASE64` | keystore `.jks`/`.keystore` codificada em Base64 |
| `KEYSTORE_PASSWORD` | senha da keystore |
| `KEY_ALIAS` | alias da chave |
| `KEY_PASSWORD` | senha da chave |

Sem `KEYSTORE_BASE64`, o workflow usa a chave de debug e o APK continua
instalável, mas não é adequado para publicação.

Exemplo para gerar o Base64 localmente:

```bash
base64 -w 0 minha-chave.jks > minha-chave.jks.b64
```

## Estrutura do repositório

```text
app/                       Aplicativo Android (Kotlin + Compose)
  src/main/java/...        UI, ViewModel, serviço, SAF e ponte JNI
  src/main/res/            Strings pt-BR/en, temas e ícones
pkg_core/                  Núcleo Rust (PKG, PFS, SFO, JNI)
  src/pkg.rs               Análise/extração de PKG
  src/pfs_builder.rs       Escrita de PFS/PFSC
  src/pack.rs              Criação de PKG
  src/jni_bridge.rs        Exportações JNI
.github/workflows/         CI de build e release
```

## Referências e licenças

- [`orbis-pfs`](https://crates.io/crates/orbis-pfs) — leitura de PFS
  (incluindo descriptografia XTS de imagens cifradas); licença MIT OR Apache-2.0.
- [shadPS4](https://github.com/shadps4-emu/shadPS4) — referência do algoritmo de
  derivação de chaves PFS para PKGs comerciais (`src/core/crypto/crypto.cpp`,
  `keys.h` e `src/core/file_format/pkg.cpp`, tag `v.0.7.0`). O projeto é
  GPL-2.0-or-later; os keysets RSA e o fluxo de derivação foram reimplementados
  em Rust a partir das constantes públicas documentadas no projeto e no
  [psdevwiki](https://www.psdevwiki.com/ps4/).
- [LibOrbisPkg](https://github.com/maxton/LibOrbisPkg) — usado apenas como
  **referência de formato** (PFS, PFSC, PKG e SFO). O projeto é LGPL-3.0; nenhum
  código dele foi copiado para este repositório. Se você reutilizar código dele,
  cumpra os termos da LGPL-3.0.
- [psdevwiki](https://www.psdevwiki.com/ps4/) — documentação pública do formato
  PKG/PFS.
- Jetpack Compose, AndroidX, Kotlin e Rust com suas respectivas licenças.

Este projeto está licenciado sob a licença MIT. Veja [LICENSE](LICENSE).

## Privacidade e uso responsável

Todo o processamento é local. O app não possui anúncios, telemetria ou
requisições de rede. Não publique conteúdo de terceiros nem jogos comerciais.
O usuário é responsável por possuir uma cópia legal dos arquivos utilizados e
por respeitar as leis e os termos aplicáveis.
