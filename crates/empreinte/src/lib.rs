//! L'empreinte du CONTENU des sources d'où sort un artefact.
//!
//! Elle répond à une seule question, et elle a déjà coûté deux jours : cet
//! artefact est-il celui du dépôt d'aujourd'hui ? Ni la version du paquet
//! (`0.1.0` d'une construction à l'autre) ni la date du fichier ne le disent —
//! la date dit quand on a compilé, pas ce qu'on a compilé.
//!
//! Le calcul vit ici, et pas en trois copies, parce qu'il est comparé de
//! l'extérieur : deux artefacts issus des mêmes sources doivent donner la même
//! empreinte au caractère près. Une divergence entre deux copies rendrait les
//! garde-fous rouges en permanence, donc faux, donc désarmés.
//!
//! Il reste UN miroir inévitable, en Python, dans le dépôt du backend : il
//! recalcule l'empreinte sans pouvoir compiler ce crate. Quatre choses doivent
//! y coïncider mot pour mot — les dossiers parcourus et leurs préfixes, le tri
//! par chemin relatif, la troncature au bloc de tests, et le séparateur nul.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Le contenu d'un fichier AVANT son bloc de tests.
///
/// Les tests ne changent aucun comportement : les hacher ferait rougir le
/// garde-fou sur une modification qui ne peut rien casser, et un rouge qu'on
/// apprend à ignorer ne garde plus rien. Mesuré le 2026-09-01 : retirer sept
/// fixtures périmées suffisait à déclarer la roue en retard.
///
/// La troncature au premier `#[cfg(test)]` en début de ligne tient parce que
/// chaque fichier du cœur n'en a qu'un, et en fin de fichier. Un test garde
/// cette convention, sans quoi la troncature emporterait du vrai code.
pub fn sans_les_tests(source: &str) -> &str {
    match source.find("\n#[cfg(test)]") {
        Some(i) => &source[..i + 1],
        None if source.starts_with("#[cfg(test)]") => "",
        None => source,
    }
}

/// Tous les `.rs` d'un dossier, triés par chemin relatif. Le tri est ce qui
/// rend l'empreinte reproductible : l'ordre de `read_dir` ne l'est pas.
pub fn fichiers_rs(racine: &Path, prefixe: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entrees) = fs::read_dir(racine) else { return };
    for e in entrees.flatten() {
        let chemin = e.path();
        let nom = e.file_name().to_string_lossy().to_string();
        let rel = if prefixe.is_empty() { nom.clone() } else { format!("{prefixe}/{nom}") };
        if chemin.is_dir() {
            fichiers_rs(&chemin, &rel, out);
        } else if chemin.extension().is_some_and(|x| x == "rs") {
            out.push((rel, chemin));
        }
    }
}

/// L'empreinte de plusieurs dossiers de sources, chacun sous son préfixe.
///
/// Les préfixes font partie du hachage : deux artefacts qui compilent le même
/// cœur mais un lien différent (PyO3, UniFFI) doivent avoir des empreintes
/// distinctes, sinon on validerait l'un en mesurant l'autre.
pub fn empreinte(dossiers: &[(PathBuf, &str)]) -> String {
    let mut fichiers = Vec::new();
    for (racine, prefixe) in dossiers {
        fichiers_rs(racine, prefixe, &mut fichiers);
    }
    fichiers.sort_by(|a, b| a.0.cmp(&b.0));

    let mut h = Sha256::new();
    for (rel, chemin) in &fichiers {
        let source = fs::read_to_string(chemin).unwrap_or_default();
        h.update(rel.as_bytes());
        h.update([0u8]);
        h.update(sans_les_tests(&source).as_bytes());
        h.update([0u8]);
    }
    format!("{:x}", h.finalize()).chars().take(12).collect()
}
