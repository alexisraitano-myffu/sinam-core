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
par `scripts/split-voice-take.py`, qui pose la référence à côté de chaque
morceau. Personne ne retranscrit à la main, et la référence ne peut pas dériver
de ce qui a été dit puisqu'elle a été écrite avant.

Ce que ça demande à la lecture :

* **un silence franc entre deux captures**, une à deux secondes ;
* lire **naturellement**, comme on dicterait, pas comme on récite ;
* en cas de bafouillage, **s'arrêter, faire un vrai silence, reprendre la
  capture entière**. L'alignement absorbe la reprise ;
* ne pas annoncer les numéros à voix haute.

**Le découpage se fait par le contenu, pas par le rythme.** Compter les silences
paraît suffisant et ne l'est pas : une respiration au milieu d'une phrase dure
autant qu'une pause entre deux captures. Mesuré sur la première prise réelle, le
compte tombait juste (30 morceaux pour 30 captures) **en étant faux** : une
capture coupée en deux compensait deux captures collées. D'où l'ordre des
opérations : transcrire la prise entière une fois, aligner les mots entendus sur
les mots écrits, couper aux changements de capture, et seulement là recaler sur
le silence le plus proche.

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
# 1. les modèles, une fois
./scripts/fetch-whisper-model.sh base-q5_1
./scripts/fetch-whisper-model.sh small-q5_1

# 2. transcrire la prise entière (elle sert à l'alignement, pas à la mesure)
afconvert -f WAVE -d LEI16@16000 -c 1 ~/Downloads/prise.m4a /tmp/prise.wav
cargo run --release --features voice-metal --example transcribe_cli -- \
    --model ~/.synapse/models/whisper/ggml-large-v3-turbo-q5_0.bin \
    --audio /tmp/prise.wav --lang fr > /tmp/segments.tsv

# 3. découper : alignement du contenu, recalage sur les silences
./scripts/split-voice-take.py --audio /tmp/prise.wav --segments /tmp/segments.tsv \
    --captures ~/.synapse/corpus-voix/captures.tsv \
    --out ~/.synapse/corpus-voix

# 4. mesurer, plusieurs modèles d'un coup
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

Le chiffre qui décide est la ligne `écart`, en noms retrouvés grâce à
l'amorçage. Trente captures portent 40 à 50 noms vérifiés : **un écart de ±1 ou
2 ne veut rien dire**, c'est le bruit. Un amorçage qui sert se voit franchement.

Ce total se lit en deux populations, et c'est la lecture qui apprend quelque
chose. Le budget de 180 tokens n'entre pas tous les noms du graphe : sur une
base de 70 entités, une quarantaine de noms passent. Le banc sépare donc les
**noms présents dans le prompt** des **autres**.

* la première ligne mesure ce que l'amorçage achète là où il agit ;
* la seconde est le garde-fou : elle doit rester **stable**. Un amorçage qui la
  dégrade écrit des noms connus à la place d'inconnus, ce qui est l'erreur qu'il
  était censé empêcher, retournée.

Mélanger les deux noierait le gain dans les noms que le prompt ne portait pas.

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

## Le moteur du téléphone, mesuré (2026-09-03)

Android expose depuis sa version 13 tout ce qu'il faut pour poser exactement la
même question à son moteur : `EXTRA_AUDIO_SOURCE` injecte un fichier à la place
du micro, et `EXTRA_BIASING_STRINGS` souffle une liste de noms au décodeur,
c'est-à-dire l'équivalent de notre amorçage. La comparaison a donc pu se faire
sur **les mêmes octets** et avec **la même liste de noms**, sans que personne
relise le corpus à voix haute.

Les deux moteurs sont notés par **le même code** : `--hyp label=fichier.tsv`
fait entrer des transcriptions faites ailleurs dans le banc, où elles passent
par le scoreur des passes locales. Vérifié en réinjectant au banc ses propres
sorties, qui retombent au chiffre près sur les deux populations. Sans ça, on
comparerait deux façons de compter autant que deux moteurs.

### Résultat

| | erreurs sur les noms | WER |
|---|---|---|
| whisper small, nu | 56,4 % | 19,7 % |
| **whisper small, amorcé** | **41,0 %** | 21,9 % |
| Android, nu | 59,0 % | 18,7 % |
| Android, amorcé | 66,7 % | 31,8 % |

**À nu, les deux sont à égalité.** 59,0 contre 56,4, c'est un nom d'écart sur
39, sous le seuil de bruit fixé plus haut. Sur le texte ordinaire le moteur du
téléphone est même très légèrement devant. L'idée reçue selon laquelle la
transcription native serait mauvaise ne se retrouve pas.

**Ce qui départage est l'amorçage, et il ne fonctionne que d'un côté.** Il fait
gagner 6 noms à whisper et en fait perdre 3 à Android. Surtout, le garde-fou
casse : sur les noms absents de la liste soufflée, Android tombe de 7/18 à 5/18,
ce qui est exactement l'erreur que l'amorçage doit empêcher, retournée. Et son
WER passe de 18,7 % à 31,8 %, donc la liste ne perturbe pas que les noms, elle
déforme la phrase entière. L'API existe, elle est bien atteinte, elle n'est pas
ignorée : c'est le moteur qui en fait mauvais usage.

**Conclusion : on garde whisper amorcé par le graphe.** Le moteur du téléphone
reste un repli honnête pour le texte, jamais pour les noms.

### Deux pièges payés pendant la mesure

**L'audio doit être poussé à la vitesse de la parole.** Nourri à environ trois
fois cette vitesse, le décodeur en flux d'Android ne rend que la FIN de chaque
prise (« Dentiste ça. » pour une phrase entière) : ce qui arrive trop vite est
écrasé avant d'être lu. La première passe donnait un moteur catastrophique qui
n'était qu'un harnais parlant trop vite. Un banc qui nourrit un moteur plus vite
que le réel ne mesure pas ce moteur.

**Une graphie correcte peut être une faute.** Android écrit « Guérin » là où le
graphe stocke « Guerin ». Il a raison en français, et il fabrique quand même une
fiche en double. Le banc compte donc une erreur, ce qui est le bon comportement
puisque c'est bien ce que ça coûte, mais la cause est côté résolution d'entité,
pas côté transcription.

### Refaire la mesure

Le côté whisper sort du banc ; `--prompt-out` écrit la liste de noms retenue,
un nom par ligne, pour que l'autre moteur reçoive exactement la même :

```bash
cargo run --release --features voice --example voice_bench -- \
    --model ~/.synapse/models/whisper/ggml-small-q5_1.bin \
    --corpus ~/.synapse/corpus-voix --db ~/.synapse/synapse.db \
    --lang fr --brief --prompt-out /tmp/amorcage.txt
```

Le côté Android sort d'un banc instrumenté vivant dans le dépôt de l'app, qui
lit le corpus poussé sur l'appareil et rend un TSV `cas`, `nu`, `amorcé`. Deux
précautions y sont indissociables : viser le build **demo**, dont l'identifiant
distinct donne un bac à sable distinct, et lancer par `am instrument` et jamais
par `connectedAndroidTest`, qui **désinstalle l'app testée** en sortie et emporte
ses données avec elle. Payé une fois, sur un téléphone de tous les jours.

Puis les deux colonnes se rejoignent dans le même tableau :

```bash
cargo run --release --features voice --example voice_bench -- \
    --model ~/.synapse/models/whisper/ggml-small-q5_1.bin \
    --corpus ~/.synapse/corpus-voix --db ~/.synapse/synapse.db \
    --lang fr --brief --hyp android=/tmp/resultats.tsv
```

Les lignes venues de `--hyp` affichent `n/a` en temps réel : la mesure a été
faite sur une autre machine, et écrire un `0.00×` ferait passer une absence de
mesure pour une mesure.
