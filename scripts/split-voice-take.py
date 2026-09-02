#!/usr/bin/env python3
"""Découpe une prise unique en cas de corpus vocal.

Lire trente captures d'affilée coûte une minute ; les enregistrer une par une
coûte une demi-heure. Ce script fait le travail inverse : il coupe la prise aux
silences et pose, à côté de chaque morceau, la référence déjà écrite dans le
fichier de captures. Personne ne retranscrit à la main.

    ./scripts/split-voice-take.py --audio prise.m4a \\
        --captures ~/.synapse/corpus-voix/captures.tsv \\
        --out ~/.synapse/corpus-voix

Le fichier de captures est un TSV : numéro, texte lu, formes à vérifier
(facultatif, séparées par des virgules). Les lignes vides et celles qui
commencent par # sont ignorées.

Le découpage n'est pas un pari : si le nombre de morceaux ne tombe pas sur le
nombre de captures, le script REFUSE d'écrire et affiche les silences trouvés
avec le réglage à essayer. Écrire des paires décalées serait pire que ne rien
écrire, parce que la mesure resterait crédible en étant fausse.

Il enregistre aussi `_bruit-de-fond.wav`, pris dans un silence entre deux
captures. C'est ce qui sert à fabriquer les cas d'hallucination : whisper
invente sur un blanc habité, pas sur un silence numérique parfait.
"""

import argparse
import array
import os
import shutil
import subprocess
import sys
import tempfile
import unicodedata
import wave

SAMPLE_RATE = 16000
FRAME_MS = 20


def fail(msg):
    print(f"split-voice-take : {msg}", file=sys.stderr)
    sys.exit(2)


def to_wav16k(path, workdir):
    """Ramène n'importe quel enregistrement au format du décodeur."""
    if path.lower().endswith(".wav"):
        with wave.open(path, "rb") as w:
            if w.getframerate() == SAMPLE_RATE and w.getnchannels() == 1 and w.getsampwidth() == 2:
                return path
    if not shutil.which("afconvert"):
        fail("afconvert introuvable : convertir la prise en WAV 16 kHz mono d'abord")
    out = os.path.join(workdir, "prise-16k.wav")
    subprocess.run(
        ["afconvert", "-f", "WAVE", "-d", f"LEI16@{SAMPLE_RATE}", "-c", "1", path, out],
        check=True,
    )
    return out


def read_samples(path):
    with wave.open(path, "rb") as w:
        if w.getnchannels() != 1 or w.getsampwidth() != 2 or w.getframerate() != SAMPLE_RATE:
            fail(f"{path} n'est pas du WAV 16 kHz mono 16 bits")
        data = w.readframes(w.getnframes())
    samples = array.array("h")
    samples.frombytes(data)
    return samples


def frame_energies(samples):
    """Énergie moyenne par fenêtre de 20 ms, en valeur absolue (pas de racine :
    seul l'ordre de grandeur relatif au bruit de fond nous intéresse)."""
    size = SAMPLE_RATE * FRAME_MS // 1000
    out = []
    for start in range(0, len(samples) - size + 1, size):
        window = samples[start : start + size]
        out.append(sum(abs(s) for s in window) / size)
    return out


def silence_runs(energies, threshold):
    runs, start = [], None
    for i, e in enumerate(energies):
        if e < threshold:
            if start is None:
                start = i
        elif start is not None:
            runs.append((start, i))
            start = None
    if start is not None:
        runs.append((start, len(energies)))
    return runs


def slug(text, words=3):
    text = unicodedata.normalize("NFKD", text.lower())
    text = "".join(c for c in text if not unicodedata.combining(c))
    keep = []
    for word in text.split()[:words]:
        word = "".join(c for c in word if c.isalnum())
        if word:
            keep.append(word)
    return "-".join(keep) or "cas"


def load_captures(path):
    rows = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) < 2 or not parts[1].strip():
                continue
            numero = parts[0].strip()
            texte = parts[1].strip()
            noms = [n.strip() for n in parts[2].split(",")] if len(parts) > 2 else []
            rows.append((numero, texte, [n for n in noms if n]))
    return rows


def write_wav(path, samples):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        w.writeframes(samples.tobytes())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--audio", required=True, help="la prise, dans n'importe quel format")
    ap.add_argument("--captures", required=True, help="TSV numéro / texte / noms")
    ap.add_argument("--out", required=True, help="dossier du corpus")
    ap.add_argument("--gap", type=float, default=1.6, help="silence séparateur, en secondes")
    ap.add_argument("--pad", type=float, default=0.25, help="marge gardée autour de chaque cas")
    ap.add_argument("--from-index", type=int, default=1, help="première capture de la prise")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    captures = load_captures(args.captures)
    if not captures:
        fail(f"aucune capture lisible dans {args.captures}")
    captures = captures[args.from_index - 1 :]

    with tempfile.TemporaryDirectory() as workdir:
        samples = read_samples(to_wav16k(args.audio, workdir))

    energies = frame_energies(samples)
    if not energies:
        fail("prise vide")
    calme = sorted(energies)
    plancher = calme[len(calme) // 5]  # 20e centile : le bruit de fond de la pièce
    seuil = max(plancher * 3.0, 60.0)
    min_frames = int(args.gap * 1000 / FRAME_MS)

    runs = silence_runs(energies, seuil)
    separateurs = [r for r in runs if (r[1] - r[0]) >= min_frames]

    duree = len(samples) / SAMPLE_RATE
    print(f"prise      : {duree:.0f} s, bruit de fond {plancher:.0f}, seuil {seuil:.0f}")
    print(f"séparateurs: {len(separateurs)} silences ≥ {args.gap} s")
    print(f"captures   : {len(captures)} attendues")

    # Coupe au milieu de chaque séparateur, puis on retire les silences de bord.
    coupes = [((a + b) // 2) * (SAMPLE_RATE * FRAME_MS // 1000) for a, b in separateurs]
    bornes = [0] + coupes + [len(samples)]
    segments = []
    for debut, fin in zip(bornes, bornes[1:]):
        seg = samples[debut:fin]
        e = frame_energies(seg)
        parlant = [i for i, v in enumerate(e) if v >= seuil]
        if not parlant:
            continue
        size = SAMPLE_RATE * FRAME_MS // 1000
        pad = int(args.pad * SAMPLE_RATE)
        a = max(0, parlant[0] * size - pad)
        b = min(len(seg), (parlant[-1] + 1) * size + pad)
        if (b - a) / SAMPLE_RATE < 0.5:
            continue
        segments.append(seg[a:b])

    print(f"morceaux   : {len(segments)} trouvés")
    for i, seg in enumerate(segments, start=1):
        print(f"   {i:02d} : {len(seg) / SAMPLE_RATE:5.1f} s")

    if len(segments) != len(captures):
        ecarts = sorted((r[1] - r[0]) * FRAME_MS / 1000 for r in runs if (r[1] - r[0]) >= 25)
        print("\nles morceaux ne tombent pas sur les captures, rien n'est écrit.")
        print("silences les plus longs (s) :", ", ".join(f"{d:.1f}" for d in ecarts[-40:]))
        print("essayer un autre --gap, ou --from-index si la prise ne couvre pas tout.")
        sys.exit(1)

    if args.dry_run:
        print("\n--dry-run : rien n'est écrit.")
        return

    os.makedirs(args.out, exist_ok=True)
    for seg, (numero, texte, noms) in zip(segments, captures):
        base = os.path.join(args.out, f"{numero}-{slug(texte)}")
        write_wav(base + ".wav", seg)
        with open(base + ".txt", "w", encoding="utf-8") as fh:
            fh.write(texte + "\n")
        if noms:
            with open(base + ".noms", "w", encoding="utf-8") as fh:
                fh.write("\n".join(noms) + "\n")
        print(f"  {os.path.basename(base)}.wav")

    if separateurs:
        plus_long = max(separateurs, key=lambda r: r[1] - r[0])
        size = SAMPLE_RATE * FRAME_MS // 1000
        tone = samples[plus_long[0] * size : plus_long[1] * size]
        write_wav(os.path.join(args.out, "_bruit-de-fond.wav"), tone)
        print(f"  _bruit-de-fond.wav ({len(tone) / SAMPLE_RATE:.1f} s)")


if __name__ == "__main__":
    main()
