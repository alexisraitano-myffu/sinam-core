#!/usr/bin/env bash
# Les trois artefacts du cœur, en une commande.
#
# Un correctif écrit ici n'atteint aucun hôte tant que TROIS artefacts distincts
# n'ont pas été reconstruits : le .so Android + son binding Kotlin, la roue
# Python du backend, et le xcframework iOS. Trois chemins, trois environnements,
# trois pièges — et le 05/09 le réessai sur coupure réseau tournait sur deux
# hôtes sur trois sans que rien ne le dise.
#
# Chaque artefact porte désormais l'empreinte de ses sources (voir
# crates/empreinte) et un test d'hôte la compare au dépôt. Ce script est
# l'autre moitié : rendre la reconstruction assez simple pour qu'on la fasse.
#
#   scripts/build-artefacts.sh            les trois
#   scripts/build-artefacts.sh android    un seul (android | wheel | ios)
#
# Le résultat n'est PAS copié dans les dépôts consommateurs : c'est un geste
# délibéré, qui se relit dans un diff.
set -euo pipefail
cd "$(dirname "$0")/.."
RACINE="$PWD"

# cmake n'est pas dans le PATH de ce Mac, et whisper.cpp en a besoin pour TOUTE
# cible, iOS comprise. Celui du SDK Android compile très bien pour les deux —
# le message d'échec (« is `cmake` not installed? ») fait chercher du côté de la
# cible alors que c'est le PATH.
SDK_CMAKE="${SDK_CMAKE:-$(ls -d "$HOME/Library/Android/sdk/cmake/"* 2>/dev/null | sort -V | tail -1)}"

quoi="${1:-tout}"

empreinte_de() {
    # Le marqueur est terminé par :FIN — les chaînes d'un binaire n'ont pas de
    # fin, et les octets voisins allongeaient la lecture de caractères hexa qui
    # ne lui appartenaient pas.
    strings -a "$1" 2>/dev/null \
        | grep -oE 'SINAM-EMPREINTE-SOURCE:[0-9a-f]{12}:FIN' | sort -u | head -1
}

if [[ "$quoi" == tout || "$quoi" == android ]]; then
    echo "══ Android ══"
    scripts/build-android.sh
    echo "empreinte : $(empreinte_de bindings/android/jniLibs/arm64-v8a/libsinam_core_ffi.so)"
fi

if [[ "$quoi" == tout || "$quoi" == wheel ]]; then
    echo "══ roue Python ══"
    # maturin vit dans l'environnement du backend, pas ici.
    MATURIN="${MATURIN:-$HOME/sinam/sinam-backend/.venv/bin/maturin}"
    [ -x "$MATURIN" ] || { echo "maturin introuvable ($MATURIN)" >&2; exit 1; }
    (cd crates/sinam-core-py && "$MATURIN" build --release)
    echo "roue : $(ls -t target/wheels/sinam_core-*.whl | head -1)"
    echo "  installer :  \$HOME/sinam/sinam-backend/.venv/bin/pip install \\"
    echo "                 --force-reinstall --no-deps $(ls -t "$RACINE"/target/wheels/sinam_core-*.whl | head -1)"
fi

if [[ "$quoi" == tout || "$quoi" == ios ]]; then
    echo "══ iOS ══"
    export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
    export PATH="$SDK_CMAKE/bin:$PATH"
    export ORT_IOS_XCFWK_PATH="${ORT_IOS_XCFWK_PATH:-$HOME/sinam/sinam-app/iosApp/Vendor/onnxruntime.xcframework}"
    [ -d "$ORT_IOS_XCFWK_PATH" ] || { echo "onnxruntime.xcframework introuvable ($ORT_IOS_XCFWK_PATH)" >&2; exit 1; }

    for t in aarch64-apple-ios aarch64-apple-ios-sim; do
        echo "── staticlib $t ──"
        cargo build -p sinam-core-ffi --release --no-default-features \
            --features voice-metal --target "$t"
    done

    # whisper et ggml sortent en archives séparées : l'app ne lie qu'une
    # bibliothèque, il faut donc les fondre dans la nôtre.
    for t in aarch64-apple-ios aarch64-apple-ios-sim; do
        libs=$(find "target/$t/release/build" -path '*whisper-rs-sys*/out/lib/*.a' | sort)
        [ -n "$libs" ] || { echo "aucune archive whisper pour $t" >&2; exit 1; }
        libtool -static -no_warning_for_no_symbols \
            -o "target/$t/release/libsinam_core_ffi_full.a" \
            "target/$t/release/libsinam_core_ffi.a" $libs
    done

    echo "── binding Swift ──"
    cargo build -p sinam-core-ffi --release --features voice-metal
    cargo run -p sinam-core-ffi --release --features voice-metal --bin uniffi-bindgen -- \
        generate --library target/release/libsinam_core_ffi.dylib \
        --language swift --out-dir target/swift
    # La surface UniFFI dépend des features : un binding généré sans `voice`
    # sort sans Transcriber, et l'app ne casse qu'à la compilation Xcode.
    grep -q "class Transcriber" target/swift/*.swift || { echo "binding Swift sans Transcriber" >&2; exit 1; }

    echo "── assemblage ──"
    rm -rf target/SinamCore.xcframework target/xc-headers
    mkdir -p target/xc-headers
    cp target/swift/*FFI.h target/xc-headers/
    cp target/swift/*FFI.modulemap target/xc-headers/module.modulemap
    xcodebuild -create-xcframework \
        -library target/aarch64-apple-ios/release/libsinam_core_ffi_full.a -headers target/xc-headers \
        -library target/aarch64-apple-ios-sim/release/libsinam_core_ffi_full.a -headers target/xc-headers \
        -output target/SinamCore.xcframework
    echo "empreinte : $(empreinte_de target/aarch64-apple-ios/release/libsinam_core_ffi_full.a)"
    echo "  copier :  rm -rf \$HOME/sinam/sinam-app/iosApp/Vendor/SinamCore.xcframework && \\"
    echo "            cp -R $RACINE/target/SinamCore.xcframework \$HOME/sinam/sinam-app/iosApp/Vendor/"
fi
