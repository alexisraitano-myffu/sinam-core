//! Banc de mesure de la transcription vocale, amorcée ou non par le graphe.
//!
//! ```text
//! cargo run --release --features voice --example voice_bench -- \
//!     --model ~/.synapse/models/whisper/ggml-small-q5_1.bin \
//!     --model ~/.synapse/models/whisper/ggml-base-q5_1.bin \
//!     --corpus ~/.synapse/corpus-voix \
//!     --db ~/.synapse/synapse.db \
//!     --lang fr
//! ```
//!
//! `--model` se répète : chaque modèle passe le corpus entier, nu puis amorcé,
//! et le tableau final les met côte à côte. C'est comme ça qu'on choisit celui
//! qui ira sur le téléphone, où il n'y aura ni Metal ni gros modèle : lancer
//! avec la feature `voice` nue (processeur seul) donne le facteur temps réel
//! le plus proche de ce que fera l'appareil.
//!
//! Le corpus est un dossier de paires : `01-courses.wav` (16 kHz mono) et
//! `01-courses.txt` (ce qui a réellement été dit). Un `01-courses.noms`
//! facultatif liste, une par ligne, les formes qui doivent sortir EXACTES ;
//! sans lui, ce sont les noms du graphe présents dans l'attendu qui sont
//! vérifiés.
//!
//! La métrique qui décide est le **taux d'erreur sur les noms propres**, pas le
//! WER. Un WER de 8 % dont les fautes sont des virgules ne coûte rien ; une
//! seule faute sur un prénom crée une entité en double, en silence, et personne
//! ne la voit avant d'avoir deux fiches pour la même personne. Le WER n'est
//! affiché que comme repère.
//!
//! L'accent compte, la casse non : « Theo » et « Théo » sont deux chaînes
//! différentes dans le graphe, donc deux fiches ; « théo » et « Théo » se
//! rejoignent au moment de la résolution d'entité.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sinam_core::{PrimeOptions, TranscribeOptions, Transcriber, PRIME_TOKEN_BUDGET};

struct Args {
    models: Vec<String>,
    corpus: PathBuf,
    db: Option<String>,
    lang: Option<String>,
    threads: i32,
    json: Option<PathBuf>,
    brief: bool,
    show_prompt: bool,
    vad: Option<String>,
}

fn parse_args() -> Args {
    let mut models = Vec::new();
    let mut corpus = None;
    let mut db = None;
    let mut lang = None;
    let mut threads = 4;
    let mut json = None;
    let mut brief = false;
    let mut show_prompt = false;
    let mut vad = None;
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| fail(&format!("{flag} attend une valeur")));
        match flag.as_str() {
            "--model" => models.push(value()),
            "--corpus" => corpus = Some(PathBuf::from(value())),
            "--db" => db = Some(value()),
            "--lang" => lang = Some(value()),
            "--threads" => threads = value().parse().unwrap_or(4),
            "--json" => json = Some(PathBuf::from(value())),
            "--brief" => brief = true,
            "--show-prompt" => show_prompt = true,
            "--vad" => vad = Some(value()),
            other => fail(&format!("option inconnue : {other}")),
        }
    }
    if models.is_empty() {
        fail("--model <fichier ggml> est obligatoire (répétable)");
    }
    Args {
        models,
        corpus: corpus.unwrap_or_else(|| fail("--corpus <dossier> est obligatoire")),
        db,
        lang,
        threads,
        json,
        brief,
        show_prompt,
        vad,
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("voice_bench : {msg}");
    std::process::exit(2);
}

/// Un cas du corpus : l'audio, ce qui a été dit, et les formes à vérifier.
struct Case {
    name: String,
    wav: PathBuf,
    expected: String,
    names: Vec<String>,
}

/// Ce qu'une passe (nue ou amorcée) rend sur un cas.
struct Run {
    text: String,
    hits: usize,
    misses: Vec<String>,
    wer: f64,
    dropped: usize,
    seconds: f64,
}

/// Le bilan d'un modèle sur tout le corpus, tel qu'il entre dans le tableau
/// comparatif final.
struct Summary {
    label: String,
    names_total: usize,
    hits_nu: usize,
    hits_amorce: Option<usize>,
    /// Noms effectivement présents dans le prompt, et le reste. Un amorçage
    /// tenu par un budget n'entre pas tous les noms du graphe : mélanger les
    /// deux populations noierait le gain sur ceux qu'il porte, et cacherait la
    /// dégradation sur ceux qu'il ne porte pas.
    amorces: (usize, usize, usize),
    hors: (usize, usize, usize),
    wer_nu: f64,
    wer_amorce: Option<f64>,
    dropped_nu: usize,
    dropped_amorce: usize,
    secs_nu: f64,
    secs_amorce: f64,
    audio: f64,
}

fn main() {
    let args = parse_args();

    // Les noms du graphe servent deux fois : à amorcer le décodeur, et à
    // savoir quoi vérifier dans un cas sans fichier `.noms`.
    let graph_names: Vec<String> = match args.db.as_deref() {
        Some(path) => {
            let conn = sinam_core::connect(path).unwrap_or_else(|e| fail(&format!("db : {e}")));
            let opts = PrimeOptions::default();
            conn.voice_names(opts.max_names as u32, opts.include_aliases)
                .unwrap_or_else(|e| fail(&format!("lecture des noms : {e}")))
        }
        None => Vec::new(),
    };

    let cases = load_corpus(&args.corpus, &graph_names);
    if cases.is_empty() {
        fail("aucune paire .wav/.txt dans le corpus");
    }
    // L'audio est lu une fois : il sert à tous les modèles, et sa durée totale
    // est ce qui rend les temps comparables entre eux.
    let audio: Vec<Vec<f32>> = cases.iter().map(|c| read_wav(&c.wav)).collect();
    let audio_seconds: f64 = audio
        .iter()
        .map(|pcm| pcm.len() as f64 / sinam_core::SAMPLE_RATE_HZ as f64)
        .sum();

    println!("corpus    : {} cas, {audio_seconds:.0} s d'audio", cases.len());
    if graph_names.is_empty() {
        println!("amorçage  : AUCUN (pas de --db, la comparaison n'aura qu'une ligne par modèle)");
    } else {
        println!("amorçage  : {} noms lus dans le graphe", graph_names.len());
    }

    let mut table = Vec::new();
    let mut json_models = serde_json::Map::new();
    for model in &args.models {
        let label = short_label(model);
        let decoder = Transcriber::new(model)
            .unwrap_or_else(|e| fail(&format!("modèle {label} : {e}")))
            .with_threads(args.threads);
        let prompt = decoder.fit_prompt(&graph_names, PRIME_TOKEN_BUDGET);

        println!("\n════════ {label} ════════");
        if !prompt.is_empty() {
            println!("{} noms retenus dans le prompt", prompt.split(", ").count());
            if args.show_prompt {
                println!("{prompt}");
            }
        }
        println!();

        let mut rows = Vec::new();
        for (case, pcm) in cases.iter().zip(&audio) {
            let nu = transcribe(&decoder, pcm, case, args.lang.as_deref(), None, args.vad.as_deref());
            let amorce = if prompt.is_empty() {
                None
            } else {
                Some(transcribe(
                    &decoder,
                    pcm,
                    case,
                    args.lang.as_deref(),
                    Some(&prompt),
                    args.vad.as_deref(),
                ))
            };
            if !args.brief {
                print_case(case, &nu, amorce.as_ref());
            }
            rows.push((case, nu, amorce));
        }

        let summary = summarize(&label, &rows, audio_seconds, &prompt);
        print_totals(&summary);
        if args.json.is_some() {
            json_models.insert(label.clone(), models_json(&rows, &prompt));
        }
        table.push(summary);
    }

    print_table(&table);
    if let Some(path) = args.json.as_ref() {
        write_json(path, json_models);
    }
}

/// Le nom du fichier suffit à identifier un modèle dans le tableau ; le chemin
/// complet ne fait qu'écraser la ligne.
fn short_label(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().trim_start_matches("ggml-").to_string())
        .unwrap_or_else(|| path.to_string())
}

fn summarize(
    label: &str,
    rows: &[(&Case, Run, Option<Run>)],
    audio: f64,
    prompt: &str,
) -> Summary {
    let amorce_partout = !rows.is_empty() && rows.iter().all(|(_, _, a)| a.is_some());
    // (combien de noms, trouvés nu, trouvés amorcé) pour chaque population.
    let mut amorces = (0usize, 0usize, 0usize);
    let mut hors = (0usize, 0usize, 0usize);
    for (case, nu, amorce) in rows {
        for name in &case.names {
            let bucket = if contains_name(prompt, name) { &mut amorces } else { &mut hors };
            bucket.0 += 1;
            bucket.1 += usize::from(contains_name(&nu.text, name));
            if let Some(a) = amorce {
                bucket.2 += usize::from(contains_name(&a.text, name));
            }
        }
    }
    Summary {
        label: label.to_string(),
        names_total: rows.iter().map(|(c, _, _)| c.names.len()).sum(),
        hits_nu: rows.iter().map(|(_, n, _)| n.hits).sum(),
        hits_amorce: amorce_partout
            .then(|| rows.iter().map(|(_, _, a)| a.as_ref().unwrap().hits).sum()),
        wer_nu: rows.iter().map(|(_, n, _)| n.wer).sum::<f64>() / rows.len() as f64,
        wer_amorce: amorce_partout.then(|| {
            rows.iter().map(|(_, _, a)| a.as_ref().unwrap().wer).sum::<f64>() / rows.len() as f64
        }),
        dropped_nu: rows.iter().map(|(_, n, _)| n.dropped).sum(),
        dropped_amorce: rows
            .iter()
            .map(|(_, _, a)| a.as_ref().map_or(0, |r| r.dropped))
            .sum(),
        secs_nu: rows.iter().map(|(_, n, _)| n.seconds).sum(),
        secs_amorce: rows
            .iter()
            .map(|(_, _, a)| a.as_ref().map_or(0.0, |r| r.seconds))
            .sum(),
        amorces,
        hors,
        audio,
    }
}

fn transcribe(
    decoder: &Transcriber,
    pcm: &[f32],
    case: &Case,
    lang: Option<&str>,
    prompt: Option<&str>,
    vad: Option<&str>,
) -> Run {
    let opts = TranscribeOptions {
        language: lang.map(|l| l.to_string()),
        initial_prompt: prompt.map(|p| p.to_string()),
        vad_model_path: vad.map(|v| v.to_string()),
        ..Default::default()
    };
    let started = Instant::now();
    let out = decoder
        .transcribe(pcm, &opts)
        .unwrap_or_else(|e| fail(&format!("{} : {e}", case.name)));
    let seconds = started.elapsed().as_secs_f64();

    let mut hits = 0;
    let mut misses = Vec::new();
    for name in &case.names {
        if contains_name(&out.text, name) {
            hits += 1;
        } else {
            misses.push(name.clone());
        }
    }
    Run {
        wer: wer(&case.expected, &out.text),
        text: out.text,
        hits,
        misses,
        dropped: out.segments.iter().filter(|s| s.dropped).count(),
        seconds,
    }
}

// ── Corpus ──────────────────────────────────────────────────────────────────

fn load_corpus(dir: &Path, graph_names: &[String]) -> Vec<Case> {
    let mut cases = Vec::new();
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| fail(&format!("{dir:?} : {e}")));
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for wav in paths {
        if wav.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }
        // Un `_` en tête marque un fichier de travail (le bruit de fond gardé
        // par le découpage), pas un cas.
        if wav.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('_')) {
            continue;
        }
        let txt = wav.with_extension("txt");
        let Ok(expected) = std::fs::read_to_string(&txt) else {
            eprintln!("  (ignoré : {wav:?} n'a pas de .txt à côté)");
            continue;
        };
        let expected = expected.trim().to_string();
        // Un .txt encore vide n'est pas une erreur : le corpus se remplit à son
        // rythme, et un cas sans référence ne peut rien mesurer.
        if expected.is_empty() {
            eprintln!("  (en attente : {:?} n'a pas encore sa référence écrite)", txt);
            continue;
        }
        let names_file = wav.with_extension("noms");
        let names = match std::fs::read_to_string(&names_file) {
            Ok(raw) => raw
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            // Sans liste écrite à la main, on vérifie les noms du graphe que
            // l'attendu contient : c'est exactement la population à risque.
            Err(_) => graph_names
                .iter()
                .filter(|n| contains_name(&expected, n))
                .cloned()
                .collect(),
        };
        cases.push(Case {
            name: wav.file_stem().unwrap_or_default().to_string_lossy().to_string(),
            wav,
            expected,
            names,
        });
    }
    cases
}

/// Lecture WAV sans ré-échantillonnage : une fréquence inattendue est une
/// erreur bruyante, jamais une transcription silencieusement fausse.
fn read_wav(path: &Path) -> Vec<f32> {
    let mut reader =
        hound::WavReader::open(path).unwrap_or_else(|e| fail(&format!("{path:?} : {e}")));
    let spec = reader.spec();
    if spec.sample_rate != sinam_core::SAMPLE_RATE_HZ {
        fail(&format!(
            "{path:?} est à {} Hz, le décodeur attend {} Hz",
            spec.sample_rate,
            sinam_core::SAMPLE_RATE_HZ
        ));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(|s| s.ok()).collect(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / scale)
                .collect()
        }
    };
    if spec.channels <= 1 {
        return samples;
    }
    // Repli mono : le décodeur ne connaît que ça.
    let n = spec.channels as usize;
    samples
        .chunks(n)
        .map(|frame| frame.iter().sum::<f32>() / n as f32)
        .collect()
}

// ── Métriques ───────────────────────────────────────────────────────────────

/// Mots comparables : minuscules, ponctuation retirée, ACCENTS GARDÉS.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Le nom apparaît-il tel quel dans le texte ? Un nom composé doit sortir
/// entier et dans l'ordre : « Théo Marchand » n'est pas trouvé si le décodeur
/// n'a produit que « Théo ».
fn contains_name(text: &str, name: &str) -> bool {
    let hay = words(text);
    let needle = words(name);
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| w == needle.as_slice())
}

fn wer(expected: &str, got: &str) -> f64 {
    let a = words(expected);
    let b = words(got);
    if a.is_empty() {
        return if b.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, wa) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, wb) in b.iter().enumerate() {
            let cost = usize::from(wa != wb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()] as f64 / a.len() as f64
}

// ── Sortie ──────────────────────────────────────────────────────────────────

fn print_case(case: &Case, nu: &Run, amorce: Option<&Run>) {
    println!("── {} ({} noms à retrouver)", case.name, case.names.len());
    println!("   attendu : {}", case.expected);
    print_run("nu     ", case, nu);
    if let Some(run) = amorce {
        print_run("amorcé ", case, run);
    }
    println!();
}

fn print_run(label: &str, case: &Case, run: &Run) {
    println!(
        "   {label}: {}/{} noms · WER {:.1} % · {} rejet(s) · {:.1} s",
        run.hits,
        case.names.len(),
        run.wer * 100.0,
        run.dropped,
        run.seconds
    );
    println!("   {label}  {}", run.text);
    if !run.misses.is_empty() {
        println!("   {label}  manqués : {}", run.misses.join(", "));
    }
}

fn print_totals(s: &Summary) {
    println!("──── {} : {} noms vérifiés ────", s.label, s.names_total);
    if s.names_total == 0 {
        println!("aucun nom à vérifier : la mesure qui compte est muette, ajoutez des .noms");
    }
    println!(
        "nu      : erreurs sur les noms {:.1} % · WER moyen {:.1} % · {} rejet(s) · {:.2}× le temps réel",
        miss_rate(s.names_total, s.hits_nu),
        s.wer_nu * 100.0,
        s.dropped_nu,
        s.secs_nu / s.audio
    );
    if let (Some(hits), Some(wer)) = (s.hits_amorce, s.wer_amorce) {
        println!(
            "amorcé  : erreurs sur les noms {:.1} % · WER moyen {:.1} % · {} rejet(s) · {:.2}× le temps réel",
            miss_rate(s.names_total, hits),
            wer * 100.0,
            s.dropped_amorce,
            s.secs_amorce / s.audio
        );
        println!(
            "écart   : {:+} nom(s) retrouvés grâce à l'amorçage sur {}",
            hits as i64 - s.hits_nu as i64,
            s.names_total
        );
        println!(
            "   dont dans le prompt : {}/{} nu → {}/{} amorcé",
            s.amorces.1, s.amorces.0, s.amorces.2, s.amorces.0
        );
        println!(
            "   hors du prompt      : {}/{} nu → {}/{} amorcé  (doit rester stable)",
            s.hors.1, s.hors.0, s.hors.2, s.hors.0
        );
    }
}

/// Le tableau qui sert à choisir. Deux lignes par modèle, parce que la question
/// n'est pas seulement « lequel transcrit le mieux » mais « lequel a besoin de
/// l'amorçage, et de combien ».
fn print_table(table: &[Summary]) {
    if table.is_empty() {
        return;
    }
    let names = table[0].names_total;
    println!("\n════════ comparatif ({names} noms, {:.0} s d'audio) ════════", table[0].audio);
    println!(
        "{:<24} {:>14} {:>10} {:>16}",
        "modèle", "erreurs noms", "WER", "temps réel"
    );
    for s in table {
        println!(
            "{:<24} {:>13.1} % {:>9.1} % {:>15.2}×",
            format!("{}  nu", s.label),
            miss_rate(s.names_total, s.hits_nu),
            s.wer_nu * 100.0,
            s.secs_nu / s.audio
        );
        if let (Some(hits), Some(wer)) = (s.hits_amorce, s.wer_amorce) {
            println!(
                "{:<24} {:>13.1} % {:>9.1} % {:>15.2}×",
                format!("{}  amorcé", s.label),
                miss_rate(s.names_total, hits),
                wer * 100.0,
                s.secs_amorce / s.audio
            );
        }
    }
    println!();
    println!("{:<24} {:>14} {:>14}", "", "noms amorcés", "noms hors");
    for s in table {
        if s.hits_amorce.is_none() {
            continue;
        }
        println!(
            "{:<24} {:>6} → {:<5} {:>6} → {:<5}",
            s.label,
            format!("{}/{}", s.amorces.1, s.amorces.0),
            format!("{}", s.amorces.2),
            format!("{}/{}", s.hors.1, s.hors.0),
            format!("{}", s.hors.2),
        );
    }
    println!(
        "\nUn écart de ±1 ou 2 noms sur {names} est du bruit : un amorçage qui sert se voit franchement."
    );
    println!(
        "La colonne « hors » est le garde-fou : elle doit rester stable, un amorçage qui la dégrade invente des noms."
    );
    println!(
        "Le temps réel se lit avec la feature utilisée : `voice` nu est le proxy du téléphone, `voice-metal` ne l'est pas."
    );
}

fn miss_rate(total: usize, hits: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (total - hits) as f64 / total as f64 * 100.0
    }
}

fn models_json(rows: &[(&Case, Run, Option<Run>)], prompt: &str) -> serde_json::Value {
    let mut cases = Vec::new();
    for (case, nu, amorce) in rows {
        let mut entry = BTreeMap::new();
        entry.insert("cas".to_string(), serde_json::json!(case.name));
        entry.insert("attendu".to_string(), serde_json::json!(case.expected));
        entry.insert("noms".to_string(), serde_json::json!(case.names));
        entry.insert("nu".to_string(), run_json(nu));
        if let Some(a) = amorce {
            entry.insert("amorce".to_string(), run_json(a));
        }
        cases.push(entry);
    }
    serde_json::json!({ "amorcage": prompt, "cas": cases })
}

fn write_json(path: &Path, models: serde_json::Map<String, serde_json::Value>) {
    let out = serde_json::Value::Object(models);
    match std::fs::write(path, serde_json::to_string_pretty(&out).unwrap()) {
        Ok(()) => println!("\njson écrit dans {path:?}"),
        Err(e) => eprintln!("écriture json impossible : {e}"),
    }
}

fn run_json(run: &Run) -> serde_json::Value {
    serde_json::json!({
        "texte": run.text,
        "noms_trouves": run.hits,
        "noms_manques": run.misses,
        "wer": run.wer,
        "segments_rejetes": run.dropped,
        "secondes": run.seconds,
    })
}
