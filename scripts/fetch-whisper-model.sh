#!/usr/bin/env bash
# Récupère un modèle whisper.cpp (ggml) dans ~/.synapse/models/whisper.
#
# Le fichier modèle est de la DONNÉE : jamais commité, jamais embarqué dans le
# crate, passé en chemin au décodeur. Même règle que le modèle d'embeddings.
#
#   ./scripts/fetch-whisper-model.sh                     # large-v3-turbo-q5_0
#   ./scripts/fetch-whisper-model.sh small               # un autre modèle
#   MODEL_DIR=/ailleurs ./scripts/fetch-whisper-model.sh
set -euo pipefail

model="${1:-large-v3-turbo-q5_0}"
dir="${MODEL_DIR:-$HOME/.synapse/models/whisper}"
url="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-${model}.bin"
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
