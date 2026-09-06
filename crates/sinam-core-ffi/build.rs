//! L'empreinte des sources gravée dans le `.so` Android et dans le
//! xcframework iOS.
//!
//! Le trou qu'elle ferme : un correctif écrit dans le cœur n'atteint les hôtes
//! que si TROIS artefacts distincts sont reconstruits à la main. Le 05/09, le
//! réessai sur coupure réseau tournait sur Android et sur le Mac, et iOS ne
//! l'avait pas — code écrit, mergé, testé, simplement pas livré là-bas. Rien ne
//! le signalait, ce qui en fait le pire des états.
//!
//! La roue Python avait déjà sa garde ; celle-ci en est le pendant pour les
//! deux autres. L'empreinte doit voyager DANS le binaire : un fichier posé à
//! côté se recopie, se périme et se perd, et ne prouve donc rien sur ce qui a
//! réellement été compilé.

use std::path::PathBuf;

fn main() {
    let ici = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let coeur = ici.join("../sinam-core/src");
    let lien = ici.join("src");

    println!("cargo:rerun-if-changed={}", coeur.display());
    println!("cargo:rerun-if-changed={}", lien.display());

    let empreinte = empreinte::empreinte(&[(coeur, "sinam-core"), (lien, "sinam-core-ffi")]);
    println!("cargo:rustc-env=SINAM_EMPREINTE_SOURCE={empreinte}");
}
