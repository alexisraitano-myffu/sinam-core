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
| ~~N2-d~~ | — | **Retirée le 30/08.** Placée en amont de `N7-c`, elle gagnait par le rang et annulait la décision du 29/08 pour toute corvée formulée comme un statut. Identifiant vacant. |  |
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
règles d'alors, 34 se calculent ou se vérifient mécaniquement, et elles vivent toutes
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

---

## La confrontation au corpus (30/08)

Les 500 cas du corpus passés dans les règles, nœud par nœud. Ce qui suit est
l'écart mesuré, pas une proposition de réécriture : rien n'a été changé ni dans
les règles ni dans le corpus.

Un mot sur ce qui est comparé. Le corpus porte l'arbitrage HUMAIN, pas la sortie
du modèle. Un écart entre une étiquette et une règle ne dit donc pas que le
modèle se trompe : il dit que les deux artefacts qui font autorité se
contredisent, et qu'un des deux est en retard sur l'autre.

### 1. Les cas que les règles ont fait changer de camp

**48 des 95 cas étiquetés « aucun souvenir » sont maintenant gardés par les
règles.** Un peu plus de la moitié tient toujours, l'autre non.

| famille | nb | ce qui a bougé |
|---|---|---|
| corvée déjà FAITE | 16 | `N2-a` retiré, `N7-c` les prend en épisode |
| séance ou moment ordinaire | 9 | `N7-c` sans condition d'intérêt |
| ressenti nu | 10 | `N7-c` ouvert aux ressentis, `N8-b` renvoie explicitement à lui |
| corvée encore À FAIRE | 7 | `N4-e`, plus aucun jugement de trivialité |
| divers | 6 | voir la liste 3 |

Les 42 premiers ne sont pas un arbitrage nouveau : ils découlent mécaniquement
des deux décisions du 29/08 (corvée future en tâche, corvée passée en épisode) et
de l'ouverture aux ressentis. Le corpus est simplement resté en arrière.
Les 6 derniers sont de vraies questions ouvertes.

Corvée faite : `x-past-errand` `r3f-episodic-past-action` `r3g-voisin-deja-vecu`
`ez-reread-notes-fr` `p-dur-action-ponctuelle-vs-durable`
`p-projf-budget-without-entity` `ord-dicte-boulangerie-garage`
`g-routine-pain-passe` `g-routine-poubelles-dicte` `r1i-course-passee-fait`
`o-tronque-voiture-fr` `ord-en-010` `ord-en-010-3` `ord-en-005-2`
`r1g-chore-nested-project` `g-status-eaten-en`

Séance ou moment : `g-type-episodic` `ep2` `g-ord-en-001` `ord-6-sport-session-fr`
`g-ord-09-sports-detail-fr` `ord-en-007-3` `ord-10-train-horaire-fr`
`p-type-animal-builtin` `ord-5-sante-dictee-fr`

Ressenti nu : `emo-bare-state-fr` `emo-bare-state-en` `emo-bare-state-duration-fr`
`emo-good-day-fr` `r1a-negatif-non-action-etat` `ord-en-006-2`
`o-en-feeling-slept-bad` `g-ord-07` `ord-en-05-3` `o-en-005`

Corvée à faire : `g-ephemeral-trivial` `p1` `p3` `r1g-chore-fr` `r1g-chore-en`
`ord-en-006` `ord-en-06`

Divers : `g-progress-decision-fr` `g-progress-metric-en`
`p-type-organisation-builtin` `r1f-action-annulee-dicte`
`g-link-restaurant-map-fr` `res-place-fr`

### 2. Les règles qu'aucun cas ne couvre

Une règle sans cas n'est pas fausse, elle est invérifiable. Deux causes très
différentes, à ne pas confondre.

**Le corpus ne sait pas mesurer ça du tout.** Aucun axe ne porte l'objet de la
règle, donc aucun cas ne pourra jamais la couvrir tant que le harnais n'ouvre pas
l'axe. Ces règles sont écrites et vivent hors de portée.

| règle | ce qu'elle demande | pourquoi c'est hors de portée |
|---|---|---|
| `N9-c` | ne pas émettre un souvenir dont la note serait vide | `note` est un booléen, il ne dit rien du texte |
| `N9-h` | ce que la note porte | idem |
| `N9-j` | le contenu éphémère, dans les mots de l'auteur | idem |
| `N9-k` | le résumé décrit sa propre note | idem |
| `N10-e` | l'ordre des souvenirs rendus | `memories` compte, il n'ordonne pas |
| `N0-a` (2e moitié) | écrire la note dans la langue de la capture | `language` mesure le champ, pas la prose |
| `G0-b` | squelette en anglais, prose dans la langue | aucun axe |
| `G4-c` | l'échelle de persistance de 5 à 1 | seule la sortie est mesurée, jamais le chiffre |

**Le corpus saurait, mais le cas n'existe pas.** Celles-là se referment en
écrivant un cas, et c'est du travail de corpus, pas de règle.

| règle | le cas qui manque | mesure |
|---|---|---|
| `N3-a` (2e moitié) | un projet douteux qui descend sous le seuil | 0 cas `proj` + `needs_review` |
| `G6-c` | une URL qu'on refuse de reformuler en fait | 0 cas `resource_url` + `forbidden_predicate` |
| `G1-g` | une URL sans nom, fiche envoyée en validation | 0 cas `resource_url` + `entity_proposed` |

**Couverture d'un ou deux cas seulement**, ce qui ne prouve pas grand-chose :
`facts_on` 1, `relation_proposed` 1, `type_proposal` 2, `renamed_to` 2,
`N9-e` 2, trois souvenirs 2.

### 3. Là où deux règles se contredisent

Sept, dont trois qui changent une sortie.

**A. `N2-d` contre `N7-c`, et c'est la plus grave.** Une corvée faite donne un
épisode, une capture qui rapporte un STATUT (« c'est fait », « c'est envoyé »)
ne laisse rien. Or « electricity bill paid », « pression des pneus faite ce
matin », « already ate » sont les deux à la fois. `N2-d` est en amont de `N7-c`,
donc il gagne, et la décision du 29/08 est annulée en silence pour toute corvée
formulée comme un statut. C'est exactement le défaut de rang que ce document
existe pour empêcher. Six cas au moins.

**B. `N1-d` contre `N2-f`.** « le restaurant Chez Léon, très bon » avec un lien :
`N2-f` est écrit pour ce cas et dit de ne rien garder, mais `N1-d` (l'auteur
prend position) se lit AVANT et garde. Le corpus tranche pour `N2-f`, la règle
tranche pour `N1-d`. Deux cas.

**C. `N1-d` n'a pas de nœud d'accueil.** Une prise de position sur une personne
ou une entreprise est gardée par `N1-d`, puis ne trouve aucune règle en `N8` :
`N8-e` ne couvre que l'œuvre, l'auteur, l'idée extérieure. La capture retombe
donc en « aucun souvenir » après avoir été gardée. Une raison de garder qui ne
mène nulle part est pire qu'une raison absente. Cas : `p-type-organisation-builtin`.

**D. `N10-a` est démenti par les seuls cas qui la testent.** Elle demande de
descendre la confiance quand trois souvenirs survivent. Le corpus a exactement
deux cas à trois souvenirs, ce sont deux listes de courses sans rapport entre
elles, `N10-d` les rend correctement à trois, et aucun des deux ne porte
`needs_review`. La règle mettrait donc deux captures justes en validation. C'est
la réponse à la question posée en marge de `N10-a` : elle n'est pas solide sur
les cas précédents, et le corpus n'a aucun cas à quatre.

**E. `N4-b` contre `N9-f`.** « appeler le client euh non oublie j'ai pas le
temps » : `N4-b` envoie la décision d'annuler vers une note, `N9-f` dit qu'une
autocorrection reprise dans le même souffle ne remplit pas le champ d'annulation.
La seconde ne parle que du champ, mais les deux se lisent comme un désaccord sur
le souvenir. Le corpus ne garde rien, les règles gardent une note.

**F. `N1-a` promet plus qu'elle ne tient.** Son ALORS dit « garder la capture »,
et sur un anniversaire nu `N6-d` conclut ensuite à aucun souvenir. Les deux sont
compatibles, « garder » ne voulant dire ici que « ne pas s'arrêter en `N2` », mais
la formulation se lit comme une promesse de souvenir. Défaut de rédaction, à
corriger dans les neuf `N1`.

**G. `G5-c` écrit « se nuance dans la note ».** La moitié graphe ne décide jamais
s'il y a une note. La phrase empiète sur l'autre moitié et ne peut rien
garantir. Défaut de rédaction.

### 4. Les cas qu'aucune règle n'explique

Quatre, tous à la frontière du sans-verbe.

- `r3f-episodic-past-action-voisin` « Ma voiture a besoin d'un lavage » : une
  action à faire énoncée comme un état, avec un verbe conjugué. `N1-h` et `N1-i`
  exigent l'absence de verbe conjugué, aucune raison de garder ne correspond,
  aucune raison de ne rien garder non plus, donc la capture tombe en `N4-a` qui
  accepte l'action « sous n'importe quelle forme » et en fait une tâche. Le
  corpus dit rien.
- `ord-en-06` « forgot to water the balcony plants before leaving for work » :
  ni faite ni clairement à faire. Aucune règle ne dit ce que devient une corvée
  oubliée.
- `g-routine-courses-tronque` « pain beurre oeufs » : `N1-i` ferme
  explicitement la porte, donc rien. Accord avec le corpus, mais c'est un accord
  par fermeture et il se paie sur une liste de courses dictée.
- `ez-weather-fr` « Il pleut » : aucune règle ne dit de ne rien garder, c'est
  l'épuisement du graphe qui répond. Accord par épuisement.

### Ce que ça dit de la suite

Le rouge attendu est bien là, et il est concentré : la moitié des cas « aucun
souvenir » a changé de camp, et une seule règle en amont (`N2-d`) suffit à
annuler la décision principale du 29/08. Corriger `N2-d` avant d'écrire le
prompt vaut plus cher que tout le reste de cette liste.

### Ce qui a été fait le 30/08 après la confrontation

**`N2-d` retirée**, pour la raison donnée en A ci-dessus. Rien ne la remplace :
l'avancement et le statut retombent sur `N7-c`, et la marge d'interprétation
qu'on rend au modèle est assumée.

**46 cas réétiquetés** dans le corpus. Ils portent tous un `⚠` dans leur `why`,
ce qui les fait remonter en tête de l'échantillon de `revue.py` : la relecture
humaine est due et le harnais la réclamera tout seul. `valide` a été retiré sur
les 25 qui le portaient, l'arbitrage précédent ne couvrant plus la nouvelle
étiquette.

Trois choses n'ont volontairement PAS été assertées sur ces cas. `event_date` sur
les épisodes passés, que `N7-d` garantit pourtant : les vingt dates se calculent à
la main et une passe séparée coûtera moins cher qu'une erreur silencieuse.
`proj` sur les comptes rendus d'avancement, qui relève de la moitié graphe. Et
`ephemeral`, dont la valeur héritée a été corrigée dans la passe suivante, plus
bas : les six nouvelles tâches portaient encore l'ancien `true`.

**Sept cas restent ouverts** et n'ont pas été touchés, parce qu'aucun d'eux ne se
tranche mécaniquement : `g-progress-decision-fr` (une décision enfouie dans un
compte rendu), `g-progress-metric-en` (un résultat mesurable dans le même),
`p-type-organisation-builtin` (contradiction C, la prise de position sans nœud
d'accueil), `r1f-action-annulee-dicte` (contradiction E), `g-link-restaurant-map-fr`
et `res-place-fr` (contradiction B), `ord-en-06` (la corvée oubliée, liste 4).

### Ce que le code fait de l'éphémère, et que les règles interdisent

Vérifié dans `routing.rs` en cherchant à quoi sert le drapeau. Il ne se contente
pas d'ajouter un rappel de 48 h.

- Quand `is_ephemeral` est vrai, chaque souvenir qui n'est ni `task` ni `event`
  est SAUTÉ, donc la note ou l'épisode n'est jamais écrit.
- Si la capture n'a par ailleurs ni entité, ni projet, ni souvenir durable, une
  sortie rapide s'exécute : il ne reste que l'intention, qui expire en 48 h.
- Le garde-fou `laisse_une_trace`, qui recopie le contenu brut quand une capture
  n'a rien laissé, compte `is_ephemeral` comme une trace. Il ne rattrape donc
  rien ici.

`N9-b` dit l'inverse mot pour mot : ni une note ni un épisode ne sont jamais
éphémères, et dans ces deux cas il faut REMETTRE LE DRAPEAU À FAUX. Le code garde
le drapeau et jette le souvenir. Un épisode que le modèle marque éphémère à tort
est donc perdu sans trace, et le harnais de langue surveille déjà cette bascule
sous le nom « action perdue ».

Ce n'est pas une question de règle, c'est un défaut de code, et il devient plus
probable depuis que `N7-c` envoie en épisode tout ce que le modèle a l'habitude
de marquer éphémère.

---

## Le dossier de l'éphémère, retiré le 30/08

Gardé en entier pour qu'il puisse revenir. Retiré de la production, pas jeté.

### Ce que c'était

Un drapeau booléen posé par le modèle sur une capture, `is_ephemeral`. Il
marquait une intention assez triviale pour s'effacer toute seule. Sa définition,
l'ancien `N9-a`, tenait en quatre conditions à réunir ensemble : un verbe
d'action à l'infinitif ou à l'impératif visant l'auteur, une action encore en
attente, sans destinataire ni engagement ni date, et sans contenu durable. Une
seule condition manquante le mettait à faux. Trois autres règles en dépendaient :
la coexistence limitée aux tâches et aux événements (`N9-b`), la garde qui
empêchait une tâche datée d'expirer avant son échéance (`N9-i`), et le contenu du
rappel écrit dans les mots de l'auteur (`N9-j`). Une quatrième, `N9-e`, disait
d'émettre la tâche et de baisser la confiance quand on hésitait.

### Ce qu'il faisait en production

Trois effets, dont deux qui ne se lisaient nulle part dans les règles.

- Une intention était écrite dans une table dédiée, avec un TTL de 48 heures.
- Tout souvenir qui n'était ni `task` ni `event` était SAUTÉ, donc une note ou un
  épisode marqué éphémère n'était jamais écrit.
- Si la capture n'avait par ailleurs ni entité, ni projet, ni souvenir durable,
  une sortie rapide s'exécutait et il ne restait que l'intention. Le garde-fou
  qui recopie le contenu brut d'une capture qui n'a rien laissé comptait le
  drapeau comme une trace, donc il ne rattrapait rien.

Autrement dit le drapeau ne s'ajoutait pas à un souvenir, il pouvait le
remplacer. `N9-b` demandait exactement l'inverse. Le harnais de langue surveille
déjà cette bascule sous le nom « action perdue ».

### Pourquoi il est parti

Trois raisons, dans l'ordre de force.

**C'est le jugement de trivialité, sous un autre nom.** Le nœud de la
micro-course a été retiré le 29/08 parce qu'on ne pèse plus ce qui mérite de
rester. L'éphémère posait la même question au même endroit, et c'est pour ça que
sa définition n'a pas survécu à ce retrait.

**Le corpus le démentait déjà, plus qu'aucune autre règle.** 57 tâches sans date
ni destinataire : 52 à faux, 5 à vrai. 16 tâches datées à vrai, 15 à faux. Aucun
ancrage ne prédisait le drapeau. En pratique il suivait la trivialité, de façon
irrégulière : « Acheter du pain » éphémère et « Acheter un harnais » permanent,
« pick up dry cleaning » éphémère et « buy coffee beans paper towels bananas »
permanent. Les quatre conditions de `N9-a` auraient rendu les 57 éphémères, donc
elles contredisaient 52 étiquettes.

**Un mécanisme d'oubli suffit.** La décroissance oublie déjà les tâches. En
garder deux dont un seul demande un jugement, c'est garder le jugement pour rien.

### Ce que ça coûte

Plus de rappel court. La liste de tâches s'allonge encore, après l'allongement
déjà accepté pour `N4-e`. C'est le prix assumé.

### Ce qu'il faudrait pour le rétablir

Deux conditions, et la seconde est la vraie.

**Un besoin mesuré**, pas supposé : des retours de testeurs sur une liste de
tâches devenue illisible, ou un chiffre sur la part de tâches jamais rouvertes.

**Un critère qui ne soit pas un jugement.** Si l'éphémère revient, il ne peut pas
redevenir « est-ce assez trivial ». La forme qui tenait le mieux à l'examen était
l'ancrage : une tâche que rien ne retient au-delà de son exécution, ni date, ni
autre personne, ni contenu qui survive à l'action, s'efface ; un seul ancrage
suffit à la garder. Mécanique, calculable, aucune trivialité pesée. C'est là
qu'il faudrait reprendre, et il faudrait alors basculer 52 étiquettes du corpus.

Et dans tous les cas, corriger d'abord le routage : normaliser le drapeau à faux
à l'entrée quand le souvenir n'est ni tâche ni événement, plutôt que de filtrer
souvenir par souvenir, et cesser de le compter comme une trace.

---

## Une règle sur-lue, corrigée le 30/08 : `N6-d`

Trouvée en préparant la réécriture du prompt, en confrontant le nœud `N6` au
corpus.

La remarque du 29/08 disait deux choses : « si il est impossible de savoir si
c'était un événement ou un fait daté, en dessous du seuil de confiance et on
demande », puis « normalement la récurrence des anniversaires sera portée dans le
graphe par les faits sur les fiches de la personne concernée ». La première
moitié parle du SOUVENIR, la seconde de la RÉCURRENCE.

J'ai écrit la seconde comme si elle parlait du souvenir, et `N6-d` a conclu qu'une
date d'anniversaire nue ne laissait aucun souvenir. Trois conséquences, toutes
fausses.

- Elle contredisait `N6-f`, écrite depuis la PREMIÈRE moitié de la même remarque,
  dans le même nœud : l'indiscernable choisit l'événement et descend la confiance.
- Elle contredisait trois arbitrages explicites du corpus, du 25/08 (« trop
  ambigus pour trancher, on demande à l'utilisateur »), du 27/08 (« la file de
  validation est le comportement voulu, c'est l'étiquette qui avait tort ») et du
  29/08 (« le modèle a raison, deux souvenirs, récurrence à faire valider »).
- Elle rouvrait le trou que le prompt fermait explicitement : un fait n'atteint
  aucune file de validation, donc retirer la note répond à la question en
  silence.

**Corrigé.** Une date d'anniversaire nue donne un `event`, récurrence à FAUX,
confiance sous le seuil. Ce qui revient chaque année est le fait `has_birthday`
sur la fiche, pas une occurrence au calendrier.

Ce que le corpus en dit une fois la règle corrigée : sept cas passent de
`recurring` vrai à faux, tous des anniversaires de personnes. Deux gardent vrai
et c'est juste, ce sont les deux seules occasions qui reviennent EN TANT
QU'OCCASIONS : « on s'est mariés le 12 juin » et « l'anniversaire de mariage de
mes parents est le 8 août ». `N6-g` les nommait déjà.

**Ce que ça apprend sur la méthode.** Une remarque en marge qui porte sur deux
champs différents se relit comme si elle n'en visait qu'un. Le garde-fou n'est
pas de relire la remarque, c'est de confronter la règle au corpus avant de
l'écrire dans le prompt : les trois arbitrages étaient là depuis cinq jours et
disaient non.

### La passe sur les nœuds restants (30/08)

Trois trouvailles après `N6-d`, en confrontant `N3`, `N4`, `N7`, `N9` et `N10` au
corpus de la même façon.

**Une date fausse.** `p-dur-birthday-durable`, « L'anniversaire de Yanis c'est le
12 juin », portait `2026-06-12`. La phrase est au PRÉSENT, donc `N0-b` résout vers
la prochaine occurrence à venir et jamais vers celle de juin dernier, et
l'ancienne valeur en faisait de surcroît une occurrence passée que `N6-e` aurait
fait descendre en épisode. Corrigée en `2027-06-12`. Les deux cas voisins,
« le 27 juillet » et « le 2 août », étaient justes : c'est la seule date du corpus
dont le mois était déjà passé, donc la seule où l'erreur pouvait se voir.

**Vingt-six épisodes ne portaient pas leur date**, que `N7-d` garantit pourtant.
Vingt venaient des réétiquetages du matin, six étaient antérieurs. Toutes
résolues depuis le lundi 13 juillet du harnais. Une seule formulation reste
volontairement ouverte, « cette nuit », qui chevauche deux jours : le cas a été
daté sur le jour du récit, que la capture nomme par ailleurs.

**`N3-b` n'a aucun cas.** Elle demande qu'un projet reconnu produise UNE note
fondatrice de nature `note`. L'axe `proj` mesure autre chose : l'entrée de projet
côté graphe. Les huit cas qui portent `proj` avec un `kind` autre que `note` ne
la violent donc pas, ils ne la mesurent pas. Personne ne vérifie aujourd'hui
qu'un « nouveau projet : rénovation de l'appartement » laisse bien une note. À
ajouter à la liste des règles auxquelles il manque juste un cas, avec `N3-a`,
`G6-c` et `G1-g`.

**Ce qui est vert.** Aucun `kind` sans note (`N9-c`), aucune tâche datée
transformée en événement sur les soixante concernées (`N4-d`), plus aucun
événement à date passée (`N6-e`).

---

## Le trou qu'Alexis a trouvé le 30/08, et une deuxième sur-lecture

Question posée après sa relecture des quatre-vingts cas : « j'ai traité pas mal
d'anniversaires, mais que côté événement, pas sur la création du fait sur la
fiche associée, c'est normal ? »

Non. **Onze des vingt-cinq cas anniversaire du corpus n'assertaient rien côté
graphe**, et parmi eux les SEPT dont je venais de retirer la récurrence. Le
raisonnement du retrait était « ce qui revient chaque année est le fait
`has_birthday` sur la fiche ». Aucun de ces sept ne vérifiait que ce fait naît.
J'ai donc retiré une garantie et mis à la place une promesse que rien ne mesure.

### Ce qui a été ouvert

Un axe `fact_asserted`, pendant positif de `fact_proposed`. Les deux rejouent la
même porte de destination sur la sortie du classifieur : le modèle ne choisit
jamais entre asserter, proposer et jeter, il choisit une force de preuve et une
persistance, et la porte fait le reste. L'axe mesure donc la conséquence.

Dix cas reçoivent leur assertion : neuf en `fact_asserted`, un en
`fact_proposed`. Restent quatre sans axe graphe et c'est justifié : les trois cas
de scénario, qui mesurent un fil et pas une sortie, et l'anniversaire de mariage
des parents, dont aucune personne nommée ne porterait le fait.

### La deuxième sur-lecture, dans `G4-e`

En écrivant ces assertions, la règle s'est révélée fausse de la même façon que
`N6-d` le matin même. Elle disait : « aucun mot d'anniversaire, ou un ÂGE à la
place d'une date → pas de has_birthday du tout ». Or « Léon est né le 27 juillet
1995 » ne porte pas le mot anniversaire et énonce la date de naissance la plus
explicite qui soit. La règle la privait de son fait, contre deux cas du corpus
qui l'attendent depuis toujours.

L'échelle arbitrée le 26/08 ne parlait pas des mots employés, elle parlait de la
SOURCE de la date. Corrigée en ce sens :

- **la date est ÉNONCÉE** directement, naissance datée ou anniversaire posé avec
  son jour et son mois → `has_birthday` explicite, il n'y a rien à deviner, et le
  doute qui reste porte sur la FÊTE et non sur la date ;
- **la date est LUE SUR UNE FÊTE** → `has_birthday` implicite, donc en
  validation : une fête tombe souvent le jour même, pas toujours ;
- **aucune date, seulement un ÂGE** → pas de `has_birthday` ; en déduire l'année
  serait de l'invention déguisée en arithmétique.

### Le troisième passage, et le bon

Les deux corrections précédentes tournaient autour de la vraie règle sans la
tenir. Alexis l'a énoncée en une phrase le 30/08 : « le cas anniversaire est
complexe, on doit presque toujours demander à l'utilisateur si c'est un fait, un
événement, ou les deux, sauf dans les cas où on peut exclure l'un ou l'autre ».

Ce qui change tout, c'est l'ordre de la question. On ne se demande plus « quelle
formulation est-ce », on se demande **ce qu'on peut EXCLURE**, et la réponse par
défaut est de demander. Il n'y a que trois exclusions possibles.

| ce que dit la capture | ce qui est exclu | ce qui naît |
|---|---|---|
| une NAISSANCE datée | l'événement, personne n'assiste à une naissance passée | `has_birthday` ASSERTÉ, aucun souvenir |
| une CÉLÉBRATION nommée | rien du côté événement, il est certain | l'`event` à la date de la fête, sans récurrence, ET `has_birthday` PROPOSÉ |
| un ÂGE sans date | les deux | rien, l'âge garde son propre fait |
| une date NUE | **rien** | l'`event` sous le seuil de confiance ET `has_birthday` PROPOSÉ |

Et la récurrence cesse d'être une décision du modèle. Si l'utilisateur répond
« c'est le fait », le fait est récurrent par nature. S'il répond « c'est
l'événement », l'événement est daté une fois et ne se rejoue pas. Il peut
répondre les deux. Dans tous les cas, ce n'est pas au modèle de trancher.

**Ce que ça corrige de la correction précédente.** Une heure plus tôt le même
jour, j'avais ASSERTÉ le fait sur les huit anniversaires nus, au motif que « la
date est énoncée, il n'y a rien à deviner ». C'était faux : ce qui est énoncé est
un JOUR, pas ce que ce jour désigne. L'anniversaire nu est précisément le cas où
rien ne s'exclut. Les huit sont repassés en PROPOSÉ, et seules les trois
naissances datées gardent un fait asserté.

### Ce que les trois sur-lectures ont en commun

Les mêmes mots, le même jour, la même cause. Une remarque ou un arbitrage qui
distingue deux choses (le souvenir et la récurrence, la source d'une date et les
mots qui l'entourent, un jour et ce que ce jour désigne) se relit comme s'il n'en
distinguait qu'une, et la règle qui en sort est plus étroite ou plus large que ce
qui a été décidé. Aucune
relecture attentive ne les a trouvées. Ce qui les a trouvées, c'est la
confrontation au corpus, et pour la seconde, une question posée par quelqu'un qui
relisait autre chose.


**Et la troisième dit autre chose encore.** Les deux premières corrections ont
été faites en cherchant la bonne réponse dans les cas du corpus. Elles ont
rapproché la règle sans l'atteindre, parce que les cas montrent des sorties et
pas le critère qui les produit. Ce qui a tranché, c'est une phrase d'Alexis qui
renversait la question : ne pas demander de quelle formulation il s'agit, mais ce
qu'on peut EXCLURE. Le corpus trouve qu'une règle est fausse ; il ne dit pas
laquelle écrire à la place.

### `N10-a` : la question posée en marge, et sa réponse mesurée

La remarque était « pas sûr de ça… si jamais on est à 4 que se passe-t-il ? est-ce
que cette règle est solide sur les précédents cas ? »

Mesuré. Le corpus a exactement DEUX cas à trois souvenirs, aucun à quatre. Les
deux sont des listes de corvées sans rapport entre elles (« pick up dry cleaning,
check oil level in the car, pay water bill »), `N10-d` les rend correctement à
trois, et **aucun des deux ne porte `needs_review`**. La règle mettait donc deux
captures parfaitement justes en file de validation.

La pénalité de confiance est retirée ; le test de fusion reste. Ce qui filtre est
le test, jamais le compte, et un nombre n'est pas en lui-même un motif de doute.

La question sur quatre reste sans réponse faute de cas, et c'est honnête de le
dire : le corpus n'en a aucun. Si le comportement à quatre compte, il faut
écrire le cas d'abord.

## La première mesure du prompt réécrit (30/08)

Les 495 cas passés dans les deux moitiés réécrites : 352 conformes. Le chiffre
brut ne dit rien, puisque les étiquettes ont bougé le même jour. Ce qui compte
est la comparaison à étiquette égale, obtenue en renotant la passe de la veille
avec les étiquettes d'aujourd'hui : sur les 230 cas communs, **112 conformes
avant, 134 après**. 47 cas corrigés, 31 cassés, 65 faux des deux côtés. Le
retrait de l'axe éphémère n'explique que six des corrections.

### Ce que la mesure a trouvé, et qui n'était pas visible en relisant

**La moitié graphe émettait trente pour cent de moins qu'avant.** 259 fiches
tombées à 178, 60 faits à 42, 42 relations à 30. Trente-trois cas rendaient un
graphe entièrement vide : les fiches naissaient, avec `facts: []` et aucune
relation. Antoine et Clara, chez qui la capture situe un barbecue, existaient
comme deux noms sans rien autour.

Deux causes, et les deux sont le problème de rang appliqué à ma propre
réécriture.

La première est un bloc que j'avais ajouté, `BEFORE YOU ANSWER`, dont une phrase
disait « émettre rien est la réponse normale pour la plupart des captures ».
L'ancien prompt tenait ce discours deux fois, le mien quatre, et la quatrième
était la dernière chose lue avant de générer. Une consigne de sobriété en
position finale ne pondère pas les autres, elle les remplace.

La seconde est un trou. Mes questions 1 et 2 demandaient quelle fiche existe et
de quel type. La 3 demandait sous quelle FORME l'écrire, fait ou relation. Aucune
ne demandait **ce que la capture dit** de la fiche. Le modèle répondait aux
questions posées.

Corrigé en trois endroits. La question 3 devient « what does the capture say
about each card », prend les fiches une par une et interdit d'anticiper le filtre
de la question 4. La question 4 gagne le pont qui lui manquait, « the scene
passes, the tie remains » : une soirée n'est pas durable, le lien qu'elle révèle
l'est presque toujours, et c'est lui qu'il faut émettre. Le bloc de fin perd sa
phrase de sobriété et devient une relecture fiche par fiche, avec sa consigne
explicite : la réparation est de remplir la fiche, jamais de la retirer.

**La porte se fermait sur deux formes de capture très courantes.** Vingt-six cas
ne rendaient rien du tout, et vingt sur vingt-six tenaient en deux familles : le
lien nu commenté (« super intéressant sur la mémoire ») et la liste de courses
nue (« beurre oeufs farine chocolat noir »).

La liste était un vrai trou : ma porte n'admettait que deux formes sans verbe,
l'infinitif nu et l'état du monde, et concluait par « ces deux et rien d'autre ».
La liste de courses est une troisième forme, et le corpus est unanime dessus,
toutes étiquetées tâche les 29 et 30/08. Elle est écrite comme telle.

Le lien commenté est un cas de rang plus subtil. La ligne qui l'ouvrait existait,
mais elle disait « a link that is the thing », un terme repris de la moitié
graphe et défini nulle part dans la moitié note. Le modèle ne pouvait pas
trancher et tombait dans la liste qui ferme. La ligne énumère maintenant les
formes (article, vidéo, tutoriel, guide, liste de lecture, critique) et porte
elle-même sa frontière avec le lien qui ouvre sur une chose ayant sa propre
existence. Une frontière écrite dans la ligne qui gagne, plutôt que dans celle
qui perd.

**Le jour de la semaine tombait un cran trop loin.** Dix-sept cas. Aujourd'hui
est le lundi 13/07, le prompt le disait, et « avant vendredi » revenait au 18 au
lieu du 17, « jeudi soir » au 10 au lieu du 9. Nommer le jour avait réglé la
semaine le 25/08 ; ça ne réglait pas le jour, parce que le modèle comptait encore
lui-même. Il ne compte plus : `today_with_weekday` écrit les deux semaines qui
encadrent aujourd'hui, la suivante à partir de demain et la précédente jusqu'à
hier, et le bloc DATES dit de lire la réponse dans une ligne. Aucune des deux ne
contient aujourd'hui, ce qui rend structurelle la règle « aujourd'hui n'est
jamais la réponse ».

### Ce que les correctifs ont donné

Les 89 cas en écart sur ces trois familles, rejoués : 41 entièrement conformes,
et le total des écarts sur le lot passe de 127 à 70.

| famille | corrigés |
|---|---|
| dates | 20 sur 36, et le décalage d'exactement un jour tombe de 17 cas à 3 |
| graphe vide | 17 sur 33 |
| porte fermée | 19 sur 26 |

Un seul écart nouveau était de ma faute : la porte ouvrant enfin sur les listes,
la moitié graphe fabriquait une fiche par article de courses. C'est la
sur-extraction connue de la moitié graphe isolée, qui se manifeste dès qu'on lui
retire un frein. Un garde est écrit en question 1 : une liste d'achats ne donne
aucune fiche, ni par article ni pour la liste, et ce qui l'entoure garde les
siennes.

## Cinq étiquettes qui contredisaient les mots d'Alexis (30/08)

Trouvées en cherchant autre chose, dans les cas dont le `why` cite Alexis
verbatim. Cinq portaient un axe qui disait l'inverse de la phrase citée juste
au-dessus.

| cas | ce qu'Alexis a écrit | ce que l'axe disait |
|---|---|---|
| `r3f-episodic-past-action` | « pas besoin de validation on enregistre en episode » | `needs_review: true` |
| `r3g-voisin-deja-vecu` | « pas besoin de validation » | `needs_review: true` |
| `p-dur-action-ponctuelle-vs-durable` | « pas de validation necessaire » | `needs_review: true` |
| `g-routine-courses-tronque` | « une tache a créer, avec validation » | `note: false`, aucune tâche |
| `r3f-episodic-past-action-voisin` | « on veux une tache […] on veut une validation » | `note: false`, `needs_review: false` |

Les cinq datent de la passe de réétiquetage du 30/08 au matin, celle qui a retiré
l'éphémère. Sur les trois premiers, le drapeau a été posé exactement à l'envers
de la décision. Les axes disent maintenant ce qui est écrit ; `valide` n'a pas
été touché.

**Ce que ça apprend.** Trois des huit écarts de confiance mesurés le matin même
n'étaient pas des écarts : le modèle rendait la bonne réponse et l'étiquette
était fausse. Une passe de réétiquetage qui touche un axe en touche d'autres par
ricochet, et rien ne le signale. Le garde est bon marché et il n'existait pas :
quand un `why` cite l'utilisateur entre guillemets, les axes doivent dire la même
chose que la citation, et ça se vérifie par script.

### Le second tour, et ce que le corpus demandait sans pouvoir l'obtenir

Les 24 cas qui résistaient après le premier tour, regardés un par un, se
répartissaient en quatre causes et deux d'entre elles accusaient l'étiquette.

**Un fait attendu sans fiche où le poser.** Neuf captures demandaient
`facts_min: 1` alors que tous leurs acteurs sont des rôles ou des noms communs :
le boulanger, le conducteur de travaux, le voisin, la mutuelle, le kid, le réseau
invité, les enfants, le Paracétamol déjà exclu par `no_entity`. La règle « un nom
commun ou un rôle n'obtient jamais de fiche » est validée et elle interdit
précisément la fiche sur laquelle le fait devrait atterrir. Arbitrage d'Alexis le
30/08 : « pas de fait la dessus ». `facts_min` retiré sur les huit, la capture
restant mesurée côté note. `ordinary-comptable-dictation` est resté en dehors,
son fait ayant Axiom pour support.

**Une intention que le prompt interdit d'écrire.** `o-en-dictated-sarah-coffee`
(« she mentioned she is planning a trip to Italy ») attendait un fait sur Sarah,
alors que la question 3 interdit les prédicats d'intention parce qu'ils
deviennent faux le jour où ils se réalisent. Le corpus demandait ce que le prompt
interdit. Arbitrage : « non pas de fiche pour sarah ». Les deux axes retirés.

**Trois corrections de prompt.** La porte énonce désormais en tête que garder est
le DÉFAUT et que jeter exige une ligne de la seconde liste, l'impression que la
capture est courte ou banale n'en étant pas une. La ligne des liens qui ferme
exige que la chose ayant sa propre identité soit NOMMÉE dans la phrase, ce qui
empêche un article ou un tutoriel de s'y faire absorber. La question 3 du graphe
énumère ce qui compte comme quelque chose de dit : un lien, une prise de
position, une recommandation, une propriété.

Résultat sur les 23 cas rejoués, tous en écart avant : 14 conformes, dont **sept
par le prompt à étiquette inchangée** et sept par l'arbitrage.

### Les neuf qui restent, et ce qu'ils disent

Trois sont la même chose : une fiche à PROPOSER que le modèle omet ou crée
franchement (Thai place, Brooklyn, Catskills). Le barreau UNSURE de la question 1
existe et ne sert pas.

Deux sont des portes qui ne s'ouvrent toujours pas, « J'ai relu mes notes de la
semaine » et le mot de passe wifi. Celle-là mérite d'être notée : la capture est
citée MOT POUR MOT dans la ligne du prompt qui dit de la garder, et elle reste
jetée. Un exemple nommé ne suffit pas à renverser un jugement de trivialité.
C'est une limite à connaître avant d'écrire une quatrième formulation.

Les quatre autres sont la frontière épisode, une date, et Axiom qui vient du
contexte et non de la capture.
