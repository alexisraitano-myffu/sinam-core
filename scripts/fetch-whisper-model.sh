#!/usr/bin/env bash
# Récupère un modèle whisper.cpp (ggml) dans ~/.synapse/models/whisper.
#
# Le fichier modèle est de la DONNÉE : jamais commité, jamais embarqué dans le
# crate, passé en chemin au décodeur. Même règle que le modèle d'embeddings.
#
#   ./scripts/fetch-whisper-model.sh                     # large-v3-turbo-q5_0
#   ./scripts/fetch-whisper-model.sh small               # un autre modèle
#   ./scripts/fetch-whisper-model.sh silero-v5.1.2       # le détecteur de parole
#   MODEL_DIR=/ailleurs ./scripts/fetch-whisper-model.sh
set -euo pipefail

model="${1:-large-v3-turbo-q5_0}"
dir="${MODEL_DIR:-$HOME/.synapse/models/whisper}"
# Le détecteur de parole (VAD) vit dans un autre dépôt que les modèles de
# transcription, et c'est un fichier d'un autre ordre de grandeur (1 Mo).
case "$model" in
  silero*) url="https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-${model}.bin" ;;
  *)       url="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-${model}.bin" ;;
esac
out="${dir}/ggml-${model}.bin"

mkdir -p "$dir"
if [ -s "$out" ]; then
  echo "déjà là : $out"
  exit 0
fi

echo "téléchargement de ggml-${model}.bin vers ${dir}"
curl -L --fail --progress-bar "$url" -o "${out}.part"
mv "${out}.part" "$out"
ls -lh "$out"
