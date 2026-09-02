# Chaîne d'outils Android pour les dépendances qui se construisent par cmake
# (whisper.cpp aujourd'hui).
#
# Pourquoi un fichier à nous plutôt que celui du NDK directement : celui du NDK
# lit `ANDROID_ABI` comme une VARIABLE cmake, jamais comme une variable
# d'environnement, et le crate `cmake` ne sait pas passer de `-D` arbitraire. Il
# retombait donc sur armeabi-v7a et cassait sur « unsupported argument
# 'armv7-a' » alors que la cible est arm64. Ici l'ABI est posée avant l'include,
# donc elle est vue.
set(ANDROID_ABI arm64-v8a)
set(ANDROID_PLATFORM android-24)
set(ANDROID_STL c++_shared)

include("$ENV{ANDROID_NDK_ROOT}/build/cmake/android.toolchain.cmake")

# Les instructions que le processeur a et que le défaut n'utilise pas. Mesuré
# sur un Pixel 9a : sans elles, les noyaux quantifiés de ggml tombent sur le
# chemin lent et la transcription est plusieurs fois plus lente que le temps
# réel. `asimddp` et `i8mm` sont dans /proc/cpuinfo de tout SoC récent, et
# armv8.2-a est le plancher de tout ce qui tourne Android 12+.
add_compile_options(-march=armv8.2-a+dotprod+i8mm+fp16 -O3)
