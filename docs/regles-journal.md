# Journal des règles

Ce fichier porte ce que `regles.md` ne porte pas : d'où vient chaque règle, ce
qui l'a fait naître, ce qui reste discuté, et les remarques laissées en
relecture. **`regles.md` se lit sans ce fichier** ; celui-ci se lit à côté.

C'est aussi le document de travail : c'est lui qu'on relit et qu'on annote, la
colonne `remarques` étant faite pour ça.

---

## L'origine de chaque règle

| # | Destination | Origine | remarques |
|---|---|---|---|
| N0-a | code | La détection est calculable. |  |
| N0-b | code | ⚠ Vit dans le prompt et s'y trompe : quatre erreurs de date le 29/08, dont deux causées par un texte qui ne parle pas de dates. **Candidat n°1 au passage dans le code.** | ok mais est-ce que placer la règle si tôt de manière déterministe est possible ? avant toute analyse. imaginons plusieurs dates ou plusieurs cas ? à moins que le déterminisme ne soit juste une règle appelée ensuite. |
| N1-N2-a | prompt | 28/08, après qu'une exception enfouie dans la ligne qu'elle contredisait ait perdu contre elle. Les écarts de routage sont tombés de six à deux. |  |
| N1-N2-b | prompt | 29/08. Sans elle, « replaced the AC filter today, next replacement due in October » perd l'échéance **sans laisser de trace**. |  |
| N1-a | code | Repérable par calcul, aujourd'hui confiée au modèle. |  |
| N1-b | exemples | Reconnaître un nom est un jugement : minuscules, prénoms inconnus, acronymes. |  |
| N1-c | exemples | « Prendre la peine de situer » est un jugement. Arbitré deux fois (Apple store contre Apple store de Lyon). |  |
| N1-d | exemples | Ce que l'auteur pense est la part qu'aucun fait ne retient. |  |
| N1-e | exemples | Cette liste se lit avant celle des raisons de ne rien garder, donc l'accomplissement gagne contre le trait. |  |
| N1-f | exemples | La frontière avec N2-a est fine et se joue sur l'attente, pas sur l'action. |  |
| N1-g | exemples | — |  |
| N1-h | prompt | Découpée de son jumeau `N1-i` le 29/08 : une seule règle ne peut pas envoyer vers deux nœuds. |  |
| N1-i | prompt | Existe parce qu'aucune fiche n'existerait pour porter cette information. |  |
| N2-b | exemples | Une habitude est un savoir qui dure et qui peut cesser, pas un moment vécu. |  |
| N2-d | exemples | — |  |
| N2-e | prompt | Une note porte toujours un mouvement qu'aucun triplet ne tient, et s'il y en avait un, la liste des raisons de garder l'aurait déjà pris. `N2-c` fusionnée ici le 29/08 : un attribut est un triplet. |  |
| N2-f | prompt | — |  |
| N3-a | exemples | « Plusieurs étapes » est un jugement : « apprendre le japonais » n'a pas le mot projet dedans. |  |
| N3-b | code | — |  |
| N4-a | exemples | — |  |
| N4-b | exemples | Écrire « annuler la réunion » en tâche mettrait au backlog la chose qu'on en retire. |  |
| N4-c | code | — |  |
| N4-d | code | — |  |
| N4-e | code | Arbitré le 29/08, en remplacement du nœud de la micro-course. Une tâche de trop se coche ; une course jetée ne laisse rien. |  |
| N6-a | exemples | C'est le seul test, et il tranche mieux que n'importe quelle liste de formes. |  |
| N6-b | prompt | — |  |
| N6-c | prompt | La même chose vaut déjà pour les tâches en N4-c. |  |
| N6-d | prompt | Repris le 29/08 : l'anniversaire d'une personne appartient à sa fiche, pas au calendrier. |  |
| N6-e | code | — |  |
| N6-f | prompt | Arbitré le 29/08. C'est le seul endroit où les deux moitiés visent la même information avec deux sorties différentes. |  |
| N6-g | prompt | Arbitré le 29/08, en même temps que `N6-d`. |  |
| N7-a | prompt | — |  |
| N7-b | exemples | — |  |
| N7-c | prompt | Ouvert le 29/08, en remplacement de trois règles qui pesaient l'intérêt d'un moment : l'ancienne corvée passée, l'ancienne séance ordinaire, et l'ancien « un sentiment n'est pas un accomplissement ». |  |
| N7-d | code | — |  |
| N7-f | prompt | — |  |
| N8-b | exemples | Corrigé le 29/08 : l'état nu ne laissait rien, il laisse maintenant un épisode. |  |
| N8-c | prompt | C'est l'autre face de N4-b : le nœud tâche la refuse, le nœud note la recueille. |  |
| N8-d | exemples | Ça ne se réduit à aucun fait précisément parce que son sujet n'a pas de fiche à lui. |  |
| N8-e | prompt | — |  |
| N8-f | prompt | Écrite parce que le modèle se taisait précisément sur les captures les plus denses. |  |
| N9-a | code | Entièrement calculable. ⚠ **Ses quatre conditions décrivent la micro-course, qui n'existe plus depuis le 29/08.** Le drapeau survit, sa définition est à reprendre. |  |
| N9-b | code | Règle unique du drapeau depuis le 29/08 : `N7-e` et `N8-a` disaient la même chose à leur propre nœud et ont été supprimées. Une note ou un épisode éphémère serait perdu sans laisser de trace. |  |
| N9-c | code | — |  |
| N9-d | prompt | La longueur ne décide de rien, la lisibilité oui. |  |
| N9-e | prompt | Application directe de ÉGAL-1. |  |
| N9-f | prompt | C'est le seul champ du prompt qui ne décide de rien. Il ne concerne QUE les tâches et les événements : ce sont les deux seules natures qu'annuler retire de la vue de l'auteur, et ce qui y va à tort retire une tâche que l'auteur n'a pas abandonnée. `N9-g` fusionnée ici le 29/08. |  |
| N9-i | code | Seule décision qui survit à la suppression du nœud de la micro-course, le 29/08. |  |
| N9-h | code | Découpée le 29/08 de `N9-j` et `N9-k` : trois destinations, trois règles. |  |
| N9-j | code | — |  |
| N9-k | code | — |  |
| N10-a | prompt | Réécrite le 29/08, l'ancienne version n'avait pas de résultat. Mesure du corpus : sur 25 cas qui comptent leurs souvenirs, 19 en attendent deux, 4 un seul, 2 en attendent trois, aucun n'en attend quatre. |  |
| N10-b | exemples | Fusionner « rappeler Nadia » et « envoyer le dossier à Laurent » fait une ligne dont la clôture retire les deux, et suspend le jeudi de Laurent à l'appel de Nadia. |  |
| N10-c | exemples | Le test dans l'autre sens : est-ce que clôturer l'une laisserait l'autre debout ? |  |
| N10-d | exemples | Arbitré le 29/08, confirmé quatre fois en revue. |  |
| N10-e | code | — |  |
| G0-a | code | La seule règle dupliquée du système qui soit VERROUILLÉE. Toutes les autres duplications dérivent en silence. |  |
| G0-b | code | Les prédicats sont une interlangue, pas de la prose. |  |
| G1-a | exemples | — |  |
| G1-b | exemples | Arbitré le 29/08. Le cran du milieu existait depuis le début et n'était posé que sur 4 cas sur 270 : c'est lui qui manquait pour éviter le choix binaire. |  |
| G1-c | exemples | Arbitré le 29/08 sur les lieux puis sur le médicament. `G1-d` et `G1-f` fusionnées ici : c'étaient trois écritures du même test. ⚠ Le volet objet consommé reste absent du prompt. |  |
| G1-e | exemples | Arbitré le 29/08. ⚠ **Absente du prompt.** |  |
| G1-g | prompt | Arbitré le 29/08. ⚠ **Contredit frontalement le prompt**, qui nomme la fiche par l'URL et appelle ça honnête. |  |
| G1-h | prompt | Un changement dont les extrémités ne sont pas enregistrées perd la seule chose durable qu'il portait. |  |
| G1-i | code | — |  |
| G2-a | code | — |  |
| G2-b | prompt | Un type inventé en silence devient permanent. |  |
| G2-c | code | — |  |
| G3-a | code | — |  |
| G3-b | code | Entièrement vérifiable par le code. |  |
| G3-c | prompt | `G3-d` fusionnée ici le 29/08 : un seul déclencheur, deux temps d'une même écriture. |  |
| G3-e | code | Détectable par motif de nom. |  |
| G3-f | prompt | Un lien déduit est très probable plutôt que certain : des frères peuvent être demi-frères, des parents des beaux-parents. |  |
| G3-g | code | — |  |
| G4-a | exemples | — |  |
| G4-b | exemples | Un fait durable s'affiche sur la fiche ET dans le digest hebdomadaire tant qu'il vit. |  |
| G4-c | exemples | C'est G1-a, appliquée au fait. |  |
| G4-d | code | — |  |
| G4-e | prompt | Ni la date du jour ni le jour où l'on a fêté ne sont une date de naissance, et déduire une année de naissance d'un âge tombe à côté une fois sur deux. |  |
| G5-a | code | L'ordre EST la règle, exactement comme en N1/N2. Un marqueur de départ n'est pas une négation, c'est un remplacement. |  |
| G5-b | code | — |  |
| G5-c | prompt | Application directe de ÉGAL-1. |  |
| G5-d | code | ⚠ La proposition s'écrit bien en base, mais rien ne la présente encore à l'utilisateur. |  |
| G5-e | code | — |  |
| G6-a | code | Le laisser tomber perd la seule chose que la capture contenait. |  |
| G6-b | exemples | — |  |
| G6-c | prompt | ⚠ Se lit avec G1-g : le prompt en tire aujourd'hui qu'une fiche nommée par son URL est « honnête », l'arbitrage dit que c'est une fiche que personne ne retrouvera. |  |
| G6-d | code | — |  |
| G7-a | exemples | Même règle que `N3-a`, écrite deux fois dans deux moitiés. **Aucune divergence n'a été mesurée à ce jour** : c'est un risque de structure, pas un défaut constaté. Le bloc des dates, lui, est verrouillé par un test ; celui-ci ne l'est pas. |  |
| G7-b | exemples | C'est ce qui permet aux avancements suivants de s'y accrocher. |  |
| G7-c | code | — |  |
| G7-d | prompt | — |  |
| ÉGAL-1 | prompt | Arbitré le 29/08. Le coût d'une erreur n'est pas symétrique : une capture jetée ne laisse aucune trace, un souvenir de trop se voit et se ferme. |  |

---

## Ce que l'ancrage au graphe montre déjà

**Le nœud le plus chargé n'est plus celui qu'on croyait.** Depuis que la
micro-course a disparu, c'est `N1`, les raisons de garder, qui porte le plus de
jugement : neuf règles dont six subjectives, devant `G1` et ses sept règles dont
quatre. C'est aussi le nœud dont trois règles sont peut-être devenues inutiles
(voir la question ouverte plus bas).

**Deux endroits où l'ORDRE est la règle, et pas le contenu.** Les raisons de
garder avant les raisons de ne rien garder (`N1-N2-a`), et les quatre questions
de `G5-a`. Dans les deux cas, la même règle écrite après le point où la décision
se prend n'a aucun effet.

**Le document a pris de l'avance sur le prompt, volontairement.** Une dizaine de
règles décrivent un comportement que le moteur ne produit pas encore : la corvée
à faire en tâche (`N4-e`), la corvée faite et l'état nu en épisode (`N7-c`,
`N8-b`), l'anniversaire descendu dans le graphe (`N6-d`, `N6-g`), le doute
occasion contre fait daté (`N6-f`), la tâche datée qui ne s'évapore pas
(`N9-i`), le seuil au-delà de deux souvenirs (`N10-a`), la confiance sur le
projet douteux (`N3-a`), le nom commun et l'objet consommé qui ne font pas fiche
(`G1-e`, `G1-c`). **La prochaine mesure du corpus sera donc rouge, et ce rouge
est l'écart à combler, pas une régression.**

**Une règle contredit frontalement le prompt au lieu de le devancer** : `G1-g`.
Le prompt dit qu'une fiche nommée par son URL est honnête ; l'arbitrage dit que
c'est une fiche que personne ne retrouvera jamais. Les deux se défendent, un
seul peut rester.

**Une définition reste écrite deux fois sans verrou** : le projet, en `N3-a` et
`G7-a`, une par moitié. Aucune divergence n'a été mesurée entre elles à ce jour :
c'est un risque de structure, pas un défaut constaté. Le bloc des dates est la
seule duplication protégée par un test.

**Un tiers des règles sont déterministes, et aucune n'est dans le code.** Sur les
93 règles, 34 se calculent ou se vérifient mécaniquement, et elles vivent toutes
dans le prompt, c'est-à-dire à l'endroit exact où deux moteurs peuvent diverger.
Les sept premières à sortir : `N0-b` (les dates), `N9-a` (l'éphémère, dont la
définition est de toute façon à reprendre), `G2-c` (la garde projet), `G3-b` (les
prédicats canoniques), `G3-e` (les prédicats d'intention), `G5-a` (l'ordre des
quatre questions), `G6-a` (une URL, un item).

**Et il ne reste que 9 préférences sur 93.** Presque tout le reste est écrit
comme une garantie, ce qui ne veut pas dire que tout l'est.

**Les règles structurelles restent les seules qui n'ont jamais eu besoin d'être
rejouées** : `N1-N2-a`, `N1-N2-b`, `N8-f`, `G3-c`, `ÉGAL-1`. C'est le meilleur
argument pour écrire le prochain prompt en structure et non en conditions.

---

## Le critère de survie d'une raison de garder

`G1-a` et `G4-c` sont gardées toutes les deux : l'une estime avant le chiffre,
l'autre chiffre et tranche en dernier. La division du travail est écrite dans les
deux règles.

Reste la question des raisons de garder, et elle se vérifie sans deviner. Le
graphe dit qu'une capture qui ne correspond à AUCUNE raison de ne rien garder est
gardée de toute façon. Donc :

> **Une raison de GARDER ne mérite d'exister que si elle défend contre une raison
> de NE RIEN garder qui aurait sinon tiré.** Sinon elle ne change aucune sortie :
> la capture tombe en `N2`, n'y correspond pas, et passe au routage.

Croisé sur les neuf raisons de garder et les quatre raisons de ne rien garder qui
restent :

| raison de garder | ce contre quoi elle défend | verdict |
|---|---|---|
| `N1-a` une date | `N2-e`, une date d'anniversaire se reformule entièrement en triplet | **porteuse** |
| `N1-b` une personne nommée | rien | candidate |
| `N1-c` un lieu situé | rien | candidate |
| `N1-d` une prise de position | rien | candidate |
| `N1-e` un accomplissement | `N2-b`, « je suis matinal, j'ai réussi à être debout avant 6h » énonce aussi un trait | **porteuse** |
| `N1-f` une chose attendue qui a bougé | `N2-d`, « le devis est parti ce matin » est un statut | **porteuse** |
| `N1-g` un lien commenté | `N2-f`, le lien sans mots restants | **porteuse** |
| `N1-h` des infinitifs nus | rien | candidate |
| `N1-i` un état du monde sans verbe | `N2-e`, « cartons au sous-sol » se reformule en triplet | **porteuse** |

**Cinq raisons sur neuf sont porteuses, quatre sont candidates au retrait.** Ce
sont les quatre dont aucune raison d'en face ne menace la capture : la personne
nommée, le lieu situé, la prise de position, les infinitifs nus. Leur routage est
déjà assuré ailleurs, respectivement par `N7-a`, `N7-c`, `N8` et `N4-a`.

**Le croisement ne suffit pas à les supprimer**, et c'est la limite honnête de la
méthode : une liste ne sert pas qu'à décider, elle sert aussi à insister, et un
modèle qui ne lit plus « une personne nommée » se taira sur des captures qu'il
gardait. La suppression se mesure, elle ne se déduit pas.

**Quand la mesurer.** Pas maintenant : le prompt est une dizaine de règles en
retard sur ce document, donc une passe aujourd'hui mesurerait l'écart au prompt
et pas l'effet du retrait. L'ordre est celui-ci. Écrire le prompt à partir de ces
règles. Passer le corpus une fois : ce rouge-là est l'écart à combler. Puis une
seconde passe avec les quatre candidates retirées, et seule la différence entre
les deux répond à la question.
