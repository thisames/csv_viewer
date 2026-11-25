#!/usr/bin/env bash
# Instala o Visualizador de Planilhas para o usuário atual (sem sudo):
#   - binário em ~/.local/bin/csv_viwer
#   - ícone e atalho no menu de aplicativos
#   - "Abrir com" para CSV, TSV, Excel e ODS no gerenciador de arquivos
#
# Uso:
#   ./instalar.sh            instala
#   ./instalar.sh --padrao   instala e define como programa padrão para CSV/TSV
#   ./instalar.sh --remover  desinstala
set -euo pipefail

cd "$(dirname "$0")"

BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor"
DESKTOP="$APP_DIR/csv-viwer.desktop"

if [ "${1:-}" = "--remover" ]; then
    rm -f "$BIN_DIR/csv_viwer" "$DESKTOP"
    for s in 48 64 128 256 512; do
        rm -f "$ICON_DIR/${s}x${s}/apps/csv-viwer.png"
    done
    update-desktop-database "$APP_DIR" 2>/dev/null || true
    echo "Removido."
    exit 0
fi

echo "› Compilando a versão otimizada (pode levar 1–2 minutos na primeira vez)…"
cargo build --release

mkdir -p "$BIN_DIR" "$APP_DIR"
install -m 755 target/release/csv_viwer "$BIN_DIR/csv_viwer"

echo "› Instalando ícones…"
for s in 48 64 128 256 512; do
    mkdir -p "$ICON_DIR/${s}x${s}/apps"
    "$BIN_DIR/csv_viwer" --write-icon "$ICON_DIR/${s}x${s}/apps/csv-viwer.png" "$s"
done

echo "› Criando o atalho no menu…"
cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=Visualizador de Planilhas
GenericName=Visualizador de CSV e planilhas
Comment=Abra arquivos CSV, TSV e Excel com rapidez
Exec=$BIN_DIR/csv_viwer %F
Icon=csv-viwer
Terminal=false
Categories=Office;Spreadsheet;Viewer;
Keywords=csv;planilha;excel;tabela;tsv;dados;
MimeType=text/csv;text/x-csv;application/csv;text/tab-separated-values;application/vnd.ms-excel;application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;application/vnd.ms-excel.sheet.macroEnabled.12;application/vnd.ms-excel.sheet.binary.macroEnabled.12;application/vnd.oasis.opendocument.spreadsheet;
StartupWMClass=csv-viwer
EOF

update-desktop-database "$APP_DIR" 2>/dev/null || true
gtk-update-icon-cache -f -t "$ICON_DIR" 2>/dev/null || true

if [ "${1:-}" = "--padrao" ]; then
    xdg-mime default csv-viwer.desktop text/csv text/tab-separated-values
    echo "› Definido como programa padrão para CSV e TSV."
fi

echo
echo "Pronto! Procure por \"Visualizador de Planilhas\" no menu,"
echo "use \"Abrir com\" no gerenciador de arquivos, ou rode: csv_viwer arquivo.csv"
