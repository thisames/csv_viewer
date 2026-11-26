#!/usr/bin/env bash
# Instala o Visualizador de Planilhas no macOS para o usuário atual (sem sudo):
#   - "Visualizador de Planilhas.app" em ~/Applications (Launchpad, Spotlight, Dock)
#   - ícone do app e "Abrir com" no Finder para CSV, TSV, Excel e ODS
#
# Chamado pelo ../instalar.sh quando roda no macOS. Uso:
#   ./instalar.sh            instala
#   ./instalar.sh --padrao   instala e explica como virar o programa padrão para CSV
#   ./instalar.sh --remover  desinstala
set -euo pipefail

cd "$(dirname "$0")/.."

NOME="Visualizador de Planilhas"
ID="io.github.thisames.csv-viwer"
APP="$HOME/Applications/$NOME.app"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

if [ "${1:-}" = "--remover" ]; then
    if [ -d "$APP" ]; then
        "$LSREGISTER" -u "$APP" 2>/dev/null || true
        rm -rf "$APP"
    fi
    echo "Removido."
    exit 0
fi

echo "› Compilando a versão otimizada (pode levar 1–2 minutos na primeira vez)…"
cargo build --release

echo "› Montando o $NOME.app…"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
BUNDLE="$TMP/$NOME.app"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
install -m 755 target/release/csv_viwer "$BUNDLE/Contents/MacOS/csv_viwer"
VERSAO=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
sed -e "s/@ID@/$ID/" -e "s/@VERSAO@/$VERSAO/g" macos/Info.plist > "$BUNDLE/Contents/Info.plist"

ICONSET="$TMP/AppIcon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
    "$BUNDLE/Contents/MacOS/csv_viwer" --write-icon "$ICONSET/icon_${s}x${s}.png" "$s"
    "$BUNDLE/Contents/MacOS/csv_viwer" --write-icon "$ICONSET/icon_${s}x${s}@2x.png" "$((s * 2))"
done
iconutil -c icns "$ICONSET" -o "$BUNDLE/Contents/Resources/AppIcon.icns"

# Assinatura local (ad hoc) do pacote inteiro: o linker só assina o binário,
# e o Info.plist e o ícone ficariam de fora.
codesign --force --sign - "$BUNDLE"

mkdir -p "$(dirname "$APP")"
rm -rf "$APP"
mv "$BUNDLE" "$APP"
"$LSREGISTER" -f "$APP"

echo
echo "Pronto! Procure por \"$NOME\" no Launchpad ou no Spotlight,"
echo "ou clique com o botão direito num CSV no Finder › Abrir com."

if [ "${1:-}" = "--padrao" ]; then
    # O macOS não tem um comando oficial para trocar o app padrão; pelo Finder:
    echo
    echo "Para abrir CSV com duplo clique: no Finder, selecione um .csv,"
    echo "Cmd+I (Obter Informações) › \"Abrir com\" › $NOME › \"Alterar Tudo…\"."
fi
