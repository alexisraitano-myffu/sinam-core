//! Transcrit un fichier et rend ses segments, un par ligne.
//!
//! ```text
//! cargo run --release --features voice --example transcribe_cli -- \
//!     --model ~/.synapse/models/whisper/ggml-base-q5_1.bin \
//!     --audio prise.wav --lang fr
//! ```
//!
//! Sortie TSV : `debut_ms`, `fin_ms`, `proba_silence`, `logprob`, `rejete`,
//! `texte`. C'est ce qui permet d'aligner une prise longue sur des textes
//! connus (voir `scripts/split-voice-take.py --segments`), et c'est aussi le
//! moyen le plus court de regarder ce que le garde-fou a écarté.

use std::path::Path;

use sinam_core::{TranscribeOptions, Transcriber};

fn fail(msg: &str) -> ! {
    eprintln!("transcribe_cli : {msg}");
    std::process::exit(2);
}

fn main() {
    let mut model = None;
    let mut audio = None;
    let mut lang = None;
    let mut prompt = None;
    let mut vad = None;
    let mut threads = 4;
    let mut audio_ctx: Option<i32> = None;
    let mut chunk: Option<f32> = None;
    let mut plain = false;
    let mut carry = 0usize;
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| fail(&format!("{flag} attend une valeur")));
        match flag.as_str() {
            "--model" => model = Some(value()),
            "--audio" => audio = Some(value()),
            "--lang" => lang = Some(value()),
            "--prompt" => prompt = Some(value()),
            "--vad" => vad = Some(value()),
            "--threads" => threads = value().parse().unwrap_or(4),
            "--audio-ctx" => audio_ctx = value().parse().ok(),
            "--chunk" => chunk = value().parse().ok(),
            "--text" => plain = true,
            "--carry" => carry = value().parse().unwrap_or(0),
            other => fail(&format!("option inconnue : {other}")),
        }
    }
    let model = model.unwrap_or_else(|| fail("--model est obligatoire"));
    let audio = audio.unwrap_or_else(|| fail("--audio est obligatoire"));

    let pcm = read_wav(Path::new(&audio));
    let decoder = Transcriber::new(&model)
        .unwrap_or_else(|e| fail(&format!("modèle : {e}")))
        .with_threads(threads);
    let opts = TranscribeOptions {
        language: lang,
        initial_prompt: prompt,
        vad_model_path: vad,
        audio_ctx,
        ..Default::default()
    };

    // `--chunk` rejoue EXACTEMENT ce que fait le téléphone : la parole y est
    // découpée en tranches décodées séparément, donc chaque tranche perd le
    // contexte de la précédente. Sans ce mode, une mesure faite sur le fichier
    // entier flatte le chemin de capture au lieu de le décrire.
    let pieces: Vec<Vec<f32>> = match chunk {
        Some(seconds) if seconds > 0.0 => {
            let n = (seconds * sinam_core::SAMPLE_RATE_HZ as f32) as usize;
            pcm.chunks(n.max(1)).map(<[f32]>::to_vec).collect()
        }
        _ => vec![pcm],
    };

    let mut out = None;
    let mut textes = Vec::new();
    for piece in &pieces {
        // `--carry` rend à la tranche suivante les derniers mots de la
        // précédente. whisper tronque l'amorçage par la fin, donc ce report
        // se place APRÈS les noms : c'est lui qui doit survivre.
        let mut opts = opts.clone();
        if carry > 0 {
            let mots: Vec<&str> = textes
                .iter()
                .flat_map(|t: &String| t.split_whitespace())
                .collect();
            let queue = mots[mots.len().saturating_sub(carry)..].join(" ");
            if !queue.is_empty() {
                opts.initial_prompt = Some(match &opts.initial_prompt {
                    Some(p) => format!("{p} {queue}"),
                    None => queue,
                });
            }
        }
        let t = decoder
            .transcribe(piece, &opts)
            .unwrap_or_else(|e| fail(&format!("décodage : {e}")));
        if !t.text.trim().is_empty() {
            textes.push(t.text.trim().to_string());
        }
        out = Some(t);
    }
    let out = out.unwrap_or_else(|| fail("aucun échantillon"));

    if plain {
        println!("{}", textes.join(" "));
        return;
    }

    for seg in &out.segments {
        println!(
            "{}\t{}\t{:.3}\t{:.3}\t{}\t{}",
            seg.start_ms,
            seg.end_ms,
            seg.no_speech_prob,
            seg.avg_logprob,
            u8::from(seg.dropped),
            seg.text.trim()
        );
    }
    eprintln!(
        "{} segments, {} rejeté(s), langue {}",
        out.segments.len(),
        out.segments.iter().filter(|s| s.dropped).count(),
        out.language.unwrap_or_else(|| "?".into())
    );
}

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
    let n = spec.channels as usize;
    samples
        .chunks(n)
        .map(|frame| frame.iter().sum::<f32>() / n as f32)
        .collect()
}
