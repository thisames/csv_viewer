# Visualizador de Planilhas

Visualizador de **CSV, TSV, TXT, Excel (.xlsx, .xls, .xlsb) e OpenDocument (.ods)**,
nativo, rápido e com interface moderna (tema escuro e claro). Feito em Rust com
[egui](https://github.com/emilk/egui).

- **Rápido de verdade**: 2 milhões de linhas (155 MB) abrem em ~0,6 s; a grade
  só desenha o que está na tela, então rolar continua leve com qualquer tamanho.
- **Entende arquivos brasileiros**: detecta sozinho o separador (`;` `,` tab `|`),
  a codificação (UTF-8, Windows-1252/Latin-1, UTF-16) e números como
  `R$ 1.234,56`, `12,5%` e datas `31/12/2024`.
- **Busca sem acento**: buscar `sao paulo` encontra `São Paulo`, com destaque.
- **Ordenação por tipo** (números como números, datas como datas, vazios no fim).
- **Filtros** pelo botão direito (“só este valor”, “excluir”, “maior ou igual a…”)
  ou pelo painel lateral (contém texto, intervalo de valores/datas), mostrados
  como “chips” removíveis.
- **Painel de colunas** com tipo detectado, mini-histograma, % preenchido,
  estatísticas (mín, máx, soma, média, mediana, desvio), distribuição e
  valores mais comuns (clique para filtrar).
- **Seleção estilo planilha**: arraste ou Shift+setas; a barra de status mostra
  soma/média/mín/máx; **Ctrl+C** copia em TSV (cola direto no Excel/Sheets).
- **Conteúdo completo da célula** (Enter ou duplo clique), com JSON formatado.
- **Exportar** a visão atual (filtros, ordem e colunas visíveis) para CSV padrão
  ou para o Excel em português (`;` + UTF-8 com BOM).
- Abas para vários arquivos, abas das planilhas do Excel, arrastar e soltar,
  arquivos recentes, ocultar colunas, ajustar largura (duplo clique na borda).

## Como rodar

```bash
cargo run --release -- caminho/arquivo.csv
```

(sem argumento abre a tela inicial; dá para arrastar arquivos para a janela).

## Instalar no menu de aplicativos

```bash
./instalar.sh            # binário em ~/.local/bin, ícone e “Abrir com” para CSV/Excel
./instalar.sh --padrao   # idem, e vira o programa padrão para .csv/.tsv
./instalar.sh --remover  # desinstala
```

No **macOS** o mesmo `./instalar.sh` monta o `Visualizador de Planilhas.app` em
`~/Applications`: aparece no Launchpad e no Spotlight, e no “Abrir com” do
Finder para CSV, TSV, Excel e ODS (arquivos abertos pelo Finder viram abas na
janela que já está aberta). O macOS não tem um comando para trocar o programa
padrão; para abrir CSV com duplo clique, selecione um `.csv` no Finder,
Cmd+I › “Abrir com” › Visualizador de Planilhas › “Alterar Tudo…”.

## Atalhos

No macOS, Ctrl é Cmd.

| Atalho | Ação |
| --- | --- |
| Ctrl+O | Abrir arquivo |
| Ctrl+F | Buscar em todas as colunas (Esc limpa) |
| Ctrl+C / Ctrl+A | Copiar seleção / selecionar tudo |
| Setas, Shift+setas, Ctrl+setas | Navegar, estender seleção, ir até a borda |
| PgUp / PgDn, Ctrl+Home / Ctrl+End | Rolar página, ir ao início/fim |
| Enter | Ver conteúdo completo da célula |
| Ctrl+E | Exportar visão atual |
| Ctrl+B | Mostrar/ocultar painel de colunas |
| Ctrl+Tab / Ctrl+W | Próxima aba / fechar aba |
| Ctrl+R | Recarregar arquivo |
| Ctrl + / Ctrl − | Zoom |
| F1 | Lista de atalhos |

## Estrutura

```
src/
  main.rs        entrada, opções de linha de comando
  app.rs         janela: abas, barras, diálogos, atalhos, notificações
  document.rs    um arquivo aberto + tarefas em segundo plano
  theme.rs       paleta, fontes (Inter, JetBrains Mono, ícones Phosphor)
  icon.rs        ícone do app desenhado por código
  format.rs      números/datas no padrão brasileiro
  data/          leitura, detecção de tipos, busca, filtros, estatísticas, exportação
  ui/            grade virtualizada, painel lateral, telas iniciais, componentes
  macos.rs       arquivos abertos pelo Finder (só no macOS)
  devshot.rs     (feature `devshot`) prints automáticos para testes visuais
macos/           Info.plist e instalar.sh do pacote .app
```

Testes: `cargo test`.

Fontes incluídas: [Inter](https://rsms.me/inter/) e
[JetBrains Mono](https://www.jetbrains.com/lp/mono/) (licença SIL OFL, em
`assets/fonts/`), ícones [Phosphor](https://phosphoricons.com) (MIT).
