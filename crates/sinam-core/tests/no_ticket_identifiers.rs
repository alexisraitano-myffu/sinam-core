//! Ce dépôt est public : aucun identifiant de ticket dans les fichiers commités.
//!
//! Un identifiant nu renvoie à un tableau que le lecteur ne peut pas ouvrir, et
//! il publie la cadence d'un backlog privé. La règle est écrite dans les deux
//! `CLAUDE.md` ; ce test est ce qui l'empêche de rester une intention.
//!
//! Ce qu'il ne couvre PAS, et qui reste à la main : les messages de commit, et
//! l'historique déjà poussé.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Le motif est assemblé à l'exécution : écrit en clair, ce fichier se
/// signalerait lui-même.
fn prefixe() -> String {
    format!("{}-", "SYN")
}

/// Le seul fichier exempté : celui-ci, qui doit bien nommer ce qu'il cherche.
fn exempte(chemin: &str) -> bool {
    chemin.ends_with("tests/no_ticket_identifiers.rs")
}

fn racine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("racine du dépôt")
}

#[test]
fn aucun_identifiant_de_ticket_dans_les_fichiers_commites() {
    let racine = racine();
    let sortie = match Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&racine)
        .output()
    {
        Ok(s) if s.status.success() => s.stdout,
        // Hors d'une copie de travail git (archive, vendoring) : rien à vérifier.
        _ => return,
    };

    let motif = prefixe();
    let mut fautifs: Vec<String> = Vec::new();

    for nom in sortie.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let nom = String::from_utf8_lossy(nom).to_string();
        if exempte(&nom) {
            continue;
        }
        let octets = match std::fs::read(racine.join(&nom)) {
            Ok(o) => o,
            Err(_) => continue, // sous-module, ou fichier indexé mais absent
        };
        if octets.iter().take(8192).any(|b| *b == 0) {
            continue; // binaire
        }
        let texte = String::from_utf8_lossy(&octets);
        for (numero, ligne) in texte.lines().enumerate() {
            let mut reste = ligne;
            while let Some(i) = reste.find(&motif) {
                let suite = &reste[i + motif.len()..];
                if suite.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    fautifs.push(format!(
                        "{}:{}: {}",
                        nom,
                        numero + 1,
                        ligne.trim().chars().take(120).collect::<String>()
                    ));
                    break;
                }
                reste = suite;
            }
        }
    }

    assert!(
        fautifs.is_empty(),
        "identifiants de ticket dans des fichiers commités — écris la raison, \
         pas le numéro (cf. CLAUDE.md) :\n  {}",
        fautifs.join("\n  ")
    );
}
