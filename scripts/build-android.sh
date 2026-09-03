#!/usr/bin/env bash
# Build the Android core artifacts consumed by the app:
#   bindings/android/jniLibs/arm64-v8a/libsinam_core_ffi.so  (ort-dynamic)
#   bindings/android/kotlin/uniffi/sinam_core_ffi/sinam_core_ffi.kt
#
# The app supplies libonnxruntime.so via the onnxruntime-android AAR; the core
# is built with --features ort-dynamic so ort dlopens it by soname at runtime.
# Never set ORT_DYLIB_PATH in an Android app (extractNativeLibs=false: the .so
# does not exist on disk; a dangling path deadlocks ort rc.12).
set -euo pipefail
cd "$(dirname "$0")/.."

OUT="${1:-bindings/android}"
TARGETS=(arm64-v8a)
FEATURES="${FEATURES:-ort-dynamic,voice}"

# ── whisper.cpp pour Android ────────────────────────────────────────────────
# La feature `voice` construit whisper.cpp par cmake. Trois choses lui manquent
# quand on croise depuis un Mac, et aucune ne se signale clairement :
#
# 1. cmake lui-même. Les Command Line Tools n'en fournissent pas ; celui du SDK
#    Android fait l'affaire, avec son ninja (sinon : « CMAKE_MAKE_PROGRAM is not
#    set »).
# 2. une chaîne d'outils. `scripts/android-toolchain.cmake` pose l'ABI AVANT
#    d'inclure celle du NDK (qui lit ANDROID_ABI comme variable cmake, jamais
#    comme variable d'environnement : sans ça elle retombe sur armeabi-v7a et
#    casse sur « unsupported argument 'armv7-a' » alors que la cible est arm64)
#    et y ajoute les instructions ARM que le processeur a. Sans elles, les
#    noyaux quantifiés de ggml prennent le chemin lent : mesuré sur un Pixel 9a,
#    la transcription passe de 2,4× à 0,9× le temps réel rien qu'avec ça.
# 3. une archive `ggml-blas` vide. whisper-rs-sys teste `cfg!(target_os =
#    "macos")` DANS son build script, donc sur l'HÔTE : en croisant depuis un
#    Mac il réclame une bibliothèque que la compilation Android ne produit pas.
#    Rien ne l'appelle, une archive vide suffit à l'éditeur de liens.
SDK_CMAKE="${SDK_CMAKE:-$(ls -d "$HOME/Library/Android/sdk/cmake/"* 2>/dev/null | sort -V | tail -1)}"
NDK_DIR="${ANDROID_NDK_HOME:-$(ls -d "$HOME/Library/Android/sdk/ndk/"* 2>/dev/null | sort -V | tail -1)}"

if [[ "$FEATURES" == *voice* ]]; then
    [ -x "$SDK_CMAKE/bin/cmake" ] || { echo "cmake introuvable (SDK_CMAKE=$SDK_CMAKE)" >&2; exit 1; }
    export PATH="$SDK_CMAKE/bin:$PATH"
    export CMAKE_GENERATOR=Ninja
    export CMAKE_MAKE_PROGRAM="$SDK_CMAKE/bin/ninja"
    export ANDROID_NDK_ROOT="$NDK_DIR"
    export ANDROID_NDK="$NDK_DIR"
    export CMAKE_TOOLCHAIN_FILE="$PWD/scripts/android-toolchain.cmake"

    SHIM_DIR="$PWD/target/android-shims"
    mkdir -p "$SHIM_DIR"
    "$NDK_DIR/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-ar" \
        rcs "$SHIM_DIR/libggml-blas.a"
    # RUSTFLAGS est découpé sur les espaces, et le chemin du dépôt en contient
    # un : passer par la forme encodée, séparée par \x1f.
    export CARGO_ENCODED_RUSTFLAGS="-L$(printf '\037')native=$SHIM_DIR"
fi

for t in "${TARGETS[@]}"; do
    cargo ndk -t "$t" build -p sinam-core-ffi \
        --no-default-features --features "$FEATURES" --release
done

# The Kotlin binding is generated from a HOST build of the same crate. The
# UniFFI surface is feature-dependent since the decoder arrived: the host build
# must carry the SAME features, sinon le binding Kotlin sort sans `Transcriber`.
CARGO_ENCODED_RUSTFLAGS="" cargo build -p sinam-core-ffi --features "${FEATURES#ort-dynamic,}"
cargo run -p sinam-core-ffi --bin uniffi-bindgen -- generate \
    --library target/debug/libsinam_core_ffi.dylib \
    --language kotlin --out-dir "$OUT/kotlin"

# The Rust cdylib links against libc++_shared (tokenizers' C++ deps, et
# whisper.cpp qui lie c++_shared lui aussi) : the APK must ship it too (bitten
# on-device: dlopen "libc++_shared.so" not found). Vendor it from the NDK next
# to our .so.
SYSROOT_LIBS="$NDK_DIR/toolchains/llvm/prebuilt/darwin-x86_64/sysroot/usr/lib"

for t in "${TARGETS[@]}"; do
    case "$t" in
        arm64-v8a) rust_target=aarch64-linux-android ;;
        x86_64) rust_target=x86_64-linux-android ;;
        *) echo "unknown target $t" >&2; exit 1 ;;
    esac
    mkdir -p "$OUT/jniLibs/$t"
    cp "target/$rust_target/release/libsinam_core_ffi.so" "$OUT/jniLibs/$t/"
    cp "$SYSROOT_LIBS/$rust_target/libc++_shared.so" "$OUT/jniLibs/$t/"
done

echo "OK — artifacts in $OUT:"
find "$OUT" -type f -exec ls -lh {} \;
