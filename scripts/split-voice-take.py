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

Compter les silences ne suffit pas toujours : une respiration au milieu d'une
phrase dure autant qu'une pause entre deux captures, et le compte peut tomber
juste en étant faux (une capture coupée en deux compense une autre collée à sa
voisine). D'où le mode ALIGNEMENT, qui décide par le contenu et pas par le
rythme :

    cargo run --release --features voice --example transcribe_cli -- \
        --model <ggml> --audio prise.wav --lang fr > segments.tsv

    ./scripts/split-voice-take.py --audio prise.wav --segments segments.tsv \
        --captures captures.tsv --out ~/.synapse/corpus-voix

La prise est transcrite une fois, puis les mots entendus sont alignés sur les
mots écrits ; les frontières tombent là où le texte change de capture, et se
recalent ensuite sur le silence le plus proche. Une hésitation ou une reprise
n'y change rien : elle est absorbée par l'alignement.

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


def normalise(mot):
    mot = unicodedata.normalize("NFKD", mot.lower())
    mot = "".join(c for c in mot if not unicodedata.combining(c))
    return "".join(c for c in mot if c.isalnum())


def load_segments(path):
    """Les segments rendus par transcribe_cli : debut_ms, fin_ms, ..., texte."""
    segments = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 6 or not parts[0].strip().isdigit():
                continue
            segments.append((int(parts[0]), int(parts[1]), parts[5]))
    return segments


def hypothese_mots(segments):
    """Un mot entendu, avec l'instant où il tombe. Les temps de whisper sont au
    segment, pas au mot : on répartit linéairement dans le segment, ce qui suffit
    puisque la frontière sera recalée sur un silence juste après."""
    mots = []
    for debut, fin, texte in segments:
        bruts = [m for m in texte.split() if normalise(m)]
        if not bruts:
            continue
        pas = (fin - debut) / len(bruts)
        for i, mot in enumerate(bruts):
            mots.append((normalise(mot), debut + pas * (i + 0.5)))
    return mots


def aligne(reference, hypothese):
    """Levenshtein avec retour sur trace. Rend, pour chaque mot de la référence,
    l'instant du mot entendu en face, ou None s'il n'a pas été prononcé."""
    n, m = len(reference), len(hypothese)
    # d[i][j] = coût d'alignement des i premiers mots écrits avec les j entendus
    d = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(n + 1):
        d[i][0] = i
    for j in range(m + 1):
        d[0][j] = j
    for i in range(1, n + 1):
        ligne, prec = d[i], d[i - 1]
        ref = reference[i - 1][0]
        for j in range(1, m + 1):
            cout = 0 if ref == hypothese[j - 1][0] else 1
            ligne[j] = min(prec[j - 1] + cout, prec[j] + 1, ligne[j - 1] + 1)
    temps = [None] * n
    i, j = n, m
    while i > 0 and j > 0:
        ref = reference[i - 1][0]
        cout = 0 if ref == hypothese[j - 1][0] else 1
        if d[i][j] == d[i - 1][j - 1] + cout:
            temps[i - 1] = hypothese[j - 1][1]
            i, j = i - 1, j - 1
        elif d[i][j] == d[i - 1][j] + 1:
            i -= 1
        else:
            j -= 1
    return temps


def recale_sur_silence(ms, runs, seuil_ms=1500):
    """Ramène une frontière sur le centre du silence le plus proche. L'alignement
    donne le bon endroit à une syllabe près ; le silence donne la coupe propre."""
    if not runs:
        return ms
    centres = [((a + b) / 2) * FRAME_MS for a, b in runs]
    proche = min(centres, key=lambda c: abs(c - ms))
    return proche if abs(proche - ms) <= seuil_ms else ms


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
    ap.add_argument(
        "--segments",
        help="TSV rendu par transcribe_cli : découpe par alignement du contenu "
        "au lieu du seul rythme des silences",
    )
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
    print(f"captures   : {len(captures)} attendues")

    par_ms = SAMPLE_RATE // 1000
    if args.segments:
        # Découpe par le contenu : les mots écrits sont alignés sur les mots
        # entendus, et la frontière tombe entre la fin d'une capture et le début
        # de la suivante, recalée sur le silence le plus proche.
        hypothese = hypothese_mots(load_segments(args.segments))
        reference = [
            (normalise(mot), idx)
            for idx, (_, texte, _) in enumerate(captures)
            for mot in texte.split()
            if normalise(mot)
        ]
        print(f"alignement : {len(reference)} mots écrits, {len(hypothese)} entendus")
        temps = aligne(reference, hypothese)
        premier, dernier = {}, {}
        for (_, idx), ms in zip(reference, temps):
            if ms is None:
                continue
            premier.setdefault(idx, ms)
            dernier[idx] = ms
        muettes = [captures[i][0] for i in range(len(captures)) if i not in premier]
        if muettes:
            fail(
                "aucun mot reconnu pour la ou les captures "
                + ", ".join(muettes)
                + " : la prise ne les couvre pas, ou la transcription a dérivé"
            )
        ancrages = [r for r in runs if (r[1] - r[0]) >= 15]  # silences ≥ 0,3 s
        bornes = [0]
        for i in range(len(captures) - 1):
            milieu = (dernier[i] + premier[i + 1]) / 2
            bornes.append(int(recale_sur_silence(milieu, ancrages) * par_ms))
        bornes.append(len(samples))
    else:
        print(f"séparateurs: {len(separateurs)} silences ≥ {args.gap} s")
        # Coupe au milieu de chaque séparateur.
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
        if args.segments:
            print("un morceau est vide après recalage : vérifier la transcription fournie.")
        else:
            print("essayer --segments (découpe par le contenu), un autre --gap,")
            print("ou --from-index si la prise ne couvre pas tout.")
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
