# Corpus vocal : protocole d'enregistrement

Ce document dit comment fabriquer le corpus qui décide si l'amorçage de la
transcription par le graphe sert à quelque chose. Il ne décrit pas du code, il
décrit un enregistrement à faire une fois, à la main.

## Ce qu'on mesure, et pourquoi pas autre chose

La métrique qui tranche est le **taux d'erreur sur les noms propres**, pas le
WER global. Un WER de 8 % fait de virgules et de « euh » ne coûte rien. Une
seule faute sur un prénom coûte une **entité en double**, créée en silence :
personne ne la voit avant d'avoir deux fiches pour la même personne, et à ce
moment-là les faits, les relations et les notes sont déjà répartis entre les
deux. Le WER est affiché par le banc comme repère, il ne décide de rien.

Trois choses ne peuvent pas être simulées, d'où un enregistrement réel :

* **la voix** : une voix de synthèse articule mieux que n'importe qui, et rend
  une mesure optimiste ;
* **les noms** : ce sont ceux du graphe, personne d'autre ne les a ;
* **les conditions** : la capture vocale sert surtout en mobilité, et c'est là
  que le décodeur souffre.

## Ce qu'il faut enregistrer

**Trente captures**, courtes (5 à 25 secondes), qui ressemblent à de vraies
captures. Pas des phrases écrites pour l'exercice : ce qui serait réellement
dicté.

Répartition à respecter, chaque ligne compte :

| combien | quoi | ce que ça mesure |
|---|---|---|
| 15 | au moins un nom **déjà dans le graphe** (personne, lieu, projet) | le gain de l'amorçage, la mesure principale |
| 5 | un nom propre **absent du graphe** | le risque inverse : l'amorçage ne doit pas écrire un nom connu à la place d'un inconnu |
| 4 | aucun nom propre | l'amorçage ne doit pas dégrader une capture ordinaire |
| 3 | un blanc de plusieurs secondes au début, au milieu ou à la fin | le garde-fou d'hallucination : whisper invente du texte plausible sur un silence |
| 3 | un acronyme, un mot étranger ou du jargon | la même classe d'erreur que les noms propres |

Et sur l'ensemble : **la moitié au calme, la moitié en mobilité** (rue, voiture,
en marchant, main pas tenue devant la bouche). C'est la condition réelle.

Les cinq cas à nom absent du graphe sont ceux qui coûtent le moins à enregistrer
et qui apprennent le plus : si l'amorçage transforme un inconnu en connu, il
fabrique exactement l'erreur qu'il est censé empêcher, mais dans l'autre sens,
et il faudra le savoir avant d'aller plus loin.

## Format des fichiers

Le corpus vit **hors du dépôt** (il est public, et ce sont des données
personnelles). Emplacement conseillé : `~/.synapse/corpus-voix/`.

Par cas, deux fichiers de même nom :

```
01-devis-terrasse.wav    # audio : WAV 16 kHz, mono, 16 bits
01-devis-terrasse.txt    # ce qui a réellement été dit, écrit à la main
```

et un troisième, facultatif :

```
01-devis-terrasse.noms   # une forme par ligne, exactement comme elle doit sortir
```

Sans `.noms`, le banc vérifie les noms du graphe qu'il trouve dans le `.txt`.
Avec, il vérifie exactement ce qui est écrit dedans. Un `.noms` vaut la peine
pour les cas à nom absent du graphe : c'est là qu'on veut nommer la forme
attendue sans ambiguïté.

Le `.txt` s'écrit **tel que la phrase a été dite**, hésitations comprises. On ne
nettoie pas : ce fichier sert de référence aux deux passes, la nue et l'amorcée,
donc tout nettoyage se compense et seul le repère WER bouge.

L'accent compte, la casse non. « Theo » et « Théo » sont deux chaînes
différentes dans le graphe, donc deux fiches ; « théo » et « Théo » se
rejoignent à la résolution d'entité.

### Convertir ce que rend le téléphone

Un enregistrement de téléphone (m4a, mp3, aac) se ramène au format attendu avec
l'outil déjà présent sur le Mac, sans rien installer :

```bash
afconvert -f WAVE -d LEI16@16000 -c 1 capture.m4a 01-devis-terrasse.wav
```

Le banc **refuse** un fichier qui n'est pas à 16 kHz plutôt que de le
ré-échantillonner en silence : une fréquence fausse ne donne pas une erreur,
elle donne du charabia crédible.

## Faire tourner la mesure

```bash
./scripts/fetch-whisper-model.sh                       # une fois, ~550 Mo

cargo run --release --features voice-metal --example voice_bench -- \
    --model ~/.synapse/models/whisper/ggml-large-v3-turbo-q5_0.bin \
    --corpus ~/.synapse/corpus-voix \
    --db ~/.synapse/synapse.db \
    --lang fr \
    --json /tmp/voix.json
```

`--features voice` suffit hors Apple (processeur seul, environ deux fois et
demie plus lent). Sans `--db`, le banc ne fait que la passe nue : il n'y a alors
rien à comparer.

Chaque cas est transcrit **deux fois**, une fois nu et une fois amorcé par les
noms du graphe, et le total donne l'écart.

## Lire le résultat

Le seul chiffre qui décide est la ligne `écart`, en noms retrouvés grâce à
l'amorçage. Avec trente captures et deux à trois noms par cas, le corpus porte
environ 60 à 90 noms : **un écart de ±1 ou 2 noms ne veut rien dire**, c'est le
bruit. Un amorçage qui sert se voit franchement ou ne sert pas.

Trois lectures possibles, et les trois sont des réponses :

* l'écart est nettement positif : l'amorçage part dans le chemin de capture ;
* l'écart est nul : on garde whisper sans amorçage, et le budget de prompt
  redevient libre pour autre chose ;
* l'écart est négatif, ou les cas à nom absent du graphe se dégradent :
  l'amorçage fabrique des noms, et c'est un mécanisme à ne pas expédier en
  production.
