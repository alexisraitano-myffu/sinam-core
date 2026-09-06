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
//! Le calcul lui-même vit dans le crate `empreinte`, partagé avec le lien
//! UniFFI : deux copies finiraient par diverger, et un garde-fou qui rougit
//! sans raison est un garde-fou qu'on désarme.

use std::fs;
use std::path::PathBuf;

fn main() {
    let ici = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let coeur = ici.join("../sinam-core/src");
    let lien = ici.join("src");
    let manifeste = ici.join("../../prompts/manifest.json");

    println!("cargo:rerun-if-changed={}", coeur.display());
    println!("cargo:rerun-if-changed={}", lien.display());
    println!("cargo:rerun-if-changed={}", manifeste.display());

    let empreinte = empreinte::empreinte(&[(coeur, "sinam-core"), (lien, "sinam-core-py")]);
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
