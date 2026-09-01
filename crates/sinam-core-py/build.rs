//! L'empreinte de ce qui a été COMPILÉ, gravée dans la roue.
//!
//! Le 30/08, la roue installée dans l'environnement du backend datait d'avant
//! un changement de routage. La suite Python affichait 197 verts toute la
//! journée en validant un comportement que le cœur n'appliquait plus. Rien ne
//! pouvait le dire : `sinam_core` reste en 0.1.0 d'une construction à l'autre.
//!
//! Deux valeurs sont donc gravées ici, et le mot compte : elles doivent vivre
//! DANS la roue. Un utilisateur installe la roue sans avoir les sources ; une
//! vérification qui lirait un fichier à côté ne vérifierait rien.
//!
//! L'empreinte hache le CONTENU des sources, pas le commit git. Un hash de
//! commit dirait « à jour » sur un arbre modifié et pas encore commité, ce qui
//! est exactement la situation d'une journée de travail.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Tous les `.rs` d'un dossier, triés par chemin relatif. Le tri est ce qui
/// rend l'empreinte reproductible : l'ordre de `read_dir` ne l'est pas.
fn fichiers_rs(racine: &Path, prefixe: &str, out: &mut Vec<(String, PathBuf)>) {
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

fn main() {
    let ici = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let coeur = ici.join("../sinam-core/src");
    let lien = ici.join("src");
    let manifeste = ici.join("../../prompts/manifest.json");

    println!("cargo:rerun-if-changed={}", coeur.display());
    println!("cargo:rerun-if-changed={}", lien.display());
    println!("cargo:rerun-if-changed={}", manifeste.display());

    let mut fichiers = Vec::new();
    fichiers_rs(&coeur, "sinam-core", &mut fichiers);
    fichiers_rs(&lien, "sinam-core-py", &mut fichiers);
    fichiers.sort_by(|a, b| a.0.cmp(&b.0));

    let mut h = Sha256::new();
    for (rel, chemin) in &fichiers {
        h.update(rel.as_bytes());
        h.update([0u8]);
        h.update(fs::read(chemin).unwrap_or_default());
        h.update([0u8]);
    }
    let empreinte: String = format!("{:x}", h.finalize()).chars().take(12).collect();
    println!("cargo:rustc-env=SINAM_EMPREINTE_SOURCE={empreinte}");

    // La version des prompts ATTENDUE par ce cœur. Le backend la compare à
    // celle réellement déployée dans SYNAPSE_HOME/prompts, qu'une
    // réinstallation du binaire bundlé peut avoir fait reculer sans rien dire.
    let version = fs::read_to_string(&manifeste)
        .ok()
        .and_then(|t| {
            t.split("\"version\"").nth(1).and_then(|reste| {
                reste
                    .trim_start_matches([':', ' '])
                    .split(|c: char| !c.is_ascii_digit())
                    .find(|s| !s.is_empty())
                    .map(str::to_string)
            })
        })
        .unwrap_or_else(|| "0".into());
    println!("cargo:rustc-env=SINAM_VERSION_PROMPTS={version}");
}
