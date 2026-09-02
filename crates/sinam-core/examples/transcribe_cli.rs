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
            other => fail(&format!("option inconnue : {other}")),
        }
    }
    let model = model.unwrap_or_else(|| fail("--model est obligatoire"));
    let audio = audio.unwrap_or_else(|| fail("--audio est obligatoire"));

    let pcm = read_wav(Path::new(&audio));
    let decoder = Transcriber::new(&model)
        .unwrap_or_else(|e| fail(&format!("modèle : {e}")))
        .with_threads(threads);
    let out = decoder
        .transcribe(
            &pcm,
            &TranscribeOptions {
                language: lang,
                initial_prompt: prompt,
                vad_model_path: vad,
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| fail(&format!("décodage : {e}")));

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
