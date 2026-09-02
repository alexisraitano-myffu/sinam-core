# Corpus vocal : protocole

Ce document dit comment fabriquer le corpus qui décide si l'amorçage de la
transcription par le graphe sert à quelque chose. Il ne décrit pas du code, il
décrit un enregistrement à faire une fois.

## Ce qu'on mesure, et pourquoi pas autre chose

La métrique qui tranche est le **taux d'erreur sur les noms propres**, pas le
WER. Un WER de 8 % fait de virgules et de « euh » ne coûte rien. Une seule faute
sur un prénom coûte une **entité en double**, créée en silence : personne ne la
voit avant d'avoir deux fiches pour la même personne, et à ce moment-là les
faits, les relations et les notes sont déjà répartis entre les deux. Le WER est
affiché par le banc comme repère, il ne décide de rien.

Trois choses ne peuvent pas être simulées, d'où un enregistrement réel :

* **la voix** : une voix de synthèse articule mieux que n'importe qui, et rend
  une mesure optimiste ;
* **les noms** : ce sont ceux du graphe, personne d'autre ne les a ;
* **les conditions** : la capture vocale sert surtout en mobilité, et c'est là
  que le décodeur souffre.

## Une seule prise, lue d'affilée

Les textes sont **écrits d'avance** dans un fichier de captures, et lus les uns
après les autres dans un seul enregistrement. Le découpage est fait après coup
par `scripts/split-voice-take.py`, qui coupe aux silences et pose la référence
à côté de chaque morceau. Personne ne retranscrit à la main, et la référence ne
peut pas dériver de ce qui a été dit puisqu'elle a été écrite avant.

Ce que ça demande à la lecture :

* **environ deux secondes de silence entre deux captures**, franches. C'est le
  seul repère du découpage ;
* lire **naturellement**, comme on dicterait, pas comme on récite ;
* en cas de bafouillage, **s'arrêter, faire un vrai silence, reprendre la
  capture entière**. Le découpage rendra un morceau de trop, ça se rattrape ;
* ne pas annoncer les numéros à voix haute.

**Deux prises valent mieux qu'une** : la même liste lue au calme, puis lue
dehors en marchant. C'est la condition réelle de la capture vocale, et comparer
les deux isole exactement ce que coûte la mobilité.

### Ce que contiennent les captures écrites

| combien | quoi | ce que ça mesure |
|---|---|---|
| 16 | au moins un nom **déjà dans le graphe** (personne, lieu, projet, marque) | le gain de l'amorçage, la mesure principale |
| 5 | un nom propre **absent du graphe** | le risque inverse : l'amorçage ne doit pas écrire un nom connu à la place d'un inconnu |
| 5 | aucun nom propre | l'amorçage ne doit pas dégrader une capture ordinaire |
| 4 | un acronyme ou du jargon | la même classe d'erreur que les noms propres |

Les cinq cas à nom absent du graphe sont ceux qui coûtent le moins et qui
apprennent le plus : si l'amorçage transforme un inconnu en connu, il fabrique
exactement l'erreur qu'il est censé empêcher, mais dans l'autre sens.

Les cas d'**hallucination sur le silence** ne sont pas lus : ils se fabriquent
après coup en insérant, dans une capture déjà découpée, le bruit de fond que le
découpage a mis de côté (`_bruit-de-fond.wav`). Whisper invente sur un blanc
habité, pas sur un silence numérique parfait, donc c'est le bon matériau et il
vient de la même pièce.

## Les fichiers

Le corpus vit **hors du dépôt** (il est public, et ce sont des données
personnelles). Emplacement : `~/.synapse/corpus-voix/`.

* `captures.tsv` : numéro, texte à lire, formes à vérifier (facultatif). C'est
  la source.
* `a-lire.txt` : la même chose sans les colonnes, à lire à l'écran.
* après découpage, une paire par cas : `01-<slug>.wav` (16 kHz mono) et
  `01-<slug>.txt` (la référence), plus `01-<slug>.noms` quand des formes
  précises sont exigées.

Sans `.noms`, le banc vérifie les noms du graphe qu'il trouve dans le `.txt`.
Avec, il vérifie exactement ce qui est écrit dedans, ce qui est indispensable
pour les cas à nom absent du graphe.

L'accent compte, la casse non. « Theo » et « Théo » sont deux chaînes
différentes dans le graphe, donc deux fiches ; « théo » et « Théo » se
rejoignent à la résolution d'entité.

## Les trois commandes

```bash
# 1. découper la prise (n'importe quel format, afconvert fait le reste)
./scripts/split-voice-take.py --audio ~/Downloads/prise.m4a \
    --captures ~/.synapse/corpus-voix/captures.tsv \
    --out ~/.synapse/corpus-voix

# 2. les modèles, une fois
./scripts/fetch-whisper-model.sh base-q5_1
./scripts/fetch-whisper-model.sh small-q5_1

# 3. mesurer, plusieurs modèles d'un coup
cargo run --release --features voice --example voice_bench -- \
    --model ~/.synapse/models/whisper/ggml-base-q5_1.bin \
    --model ~/.synapse/models/whisper/ggml-small-q5_1.bin \
    --corpus ~/.synapse/corpus-voix \
    --db ~/.synapse/synapse.db \
    --lang fr --brief --json /tmp/voix.json
```

Le banc **ne fait qu'écrire dans sa sortie** : il lit les noms du graphe et ne
touche à rien. Aucune capture du corpus n'entre dans la mémoire, ni pendant la
mesure ni après.

`--features voice` (processeur seul) est le bon réglage pour juger le mobile :
c'est le seul chemin qui ressemble à ce que fera le téléphone. `voice-metal`
sert à mesurer le desktop, pas à décider du modèle embarqué.

## Lire le résultat

Le seul chiffre qui décide est la ligne `écart`, en noms retrouvés grâce à
l'amorçage. Trente captures portent 40 à 50 noms vérifiés : **un écart de ±1 ou
2 ne veut rien dire**, c'est le bruit. Un amorçage qui sert se voit franchement.

Trois lectures possibles, et les trois sont des réponses :

* l'écart est nettement positif : l'amorçage part dans le chemin de capture ;
* l'écart est nul : on garde whisper sans amorçage, et le budget de prompt
  redevient libre pour autre chose ;
* l'écart est négatif, ou les cas à nom absent du graphe se dégradent :
  l'amorçage fabrique des noms, et c'est un mécanisme à ne pas expédier en
  production.

Le tableau comparatif sert à la deuxième question, celle du modèle embarqué :
la colonne `temps réel` dit ce que la transcription coûtera sur le téléphone, et
la colonne des noms dit ce qu'on perd en descendant de taille.
