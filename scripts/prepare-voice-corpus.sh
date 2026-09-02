#!/usr/bin/env bash
# Prépare un corpus vocal à partir de ce que rend un téléphone.
#
#   ./scripts/prepare-voice-corpus.sh ~/Downloads/captures ~/.synapse/corpus-voix
#
# Chaque fichier audio (m4a, mp3, wav, aac, aiff...) devient un `.wav` 16 kHz
# mono, plus un `.txt` VIDE à côté. Le `.txt` se remplit à la main avec ce qui a
# réellement été dit : c'est la référence, elle ne peut pas être devinée par une
# machine sans devenir juge et partie. Le banc ignore les cas dont le `.txt` est
# encore vide, donc le corpus se remplit au rythme qu'on veut.
#
# Rien à installer : afconvert est livré avec macOS.
set -euo pipefail

src="${1:?usage: prepare-voice-corpus.sh <dossier source> [dossier corpus]}"
dst="${2:-$HOME/.synapse/corpus-voix}"
mkdir -p "$dst"

n=0
for f in "$src"/*; do
  [ -f "$f" ] || continue
  case "${f##*.}" in
    m4a|mp3|aac|wav|aiff|aif|caf|mp4) ;;
    *) continue ;;
  esac
  n=$((n + 1))
  base=$(basename "$f")
  slug=$(printf '%s' "${base%.*}" | tr '[:upper:] ' '[:lower:]-' | tr -cd 'a-z0-9-')
  out=$(printf '%s/%02d-%s' "$dst" "$n" "$slug")
  afconvert -f WAVE -d LEI16@16000 -c 1 "$f" "${out}.wav"
  [ -e "${out}.txt" ] || : > "${out}.txt"
  echo "  ${out##*/}.wav"
done

echo
echo "$n cas préparés dans $dst"
echo "Reste à écrire dans chaque .txt ce qui a été dit, mot pour mot."
