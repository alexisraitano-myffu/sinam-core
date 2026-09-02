//! Transcription vocale locale, et son amorçage par le graphe de l'utilisateur.
//!
//! Le module est coupé en deux à dessein :
//!
//! * l'**amorçage** (`graph_names`, `fit_names`, `graph_prompt`) est du SQL et
//!   du texte, sans dépendance au décodeur : il compile et se teste toujours,
//!   feature `voice` ou non. C'est aussi la seule moitié qui porte une décision
//!   produit ;
//! * le **décodeur** (`Transcriber`, derrière la feature `voice`) n'est qu'une
//!   liaison whisper.cpp. Le fichier modèle y est de la DONNÉE passée en
//!   chemin, exactement comme celui des embeddings.
//!
//! Ce que l'amorçage attaque : whisper se trompe sur les noms propres, et dans
//! un système dont toute la valeur est le graphe d'entités, un prénom mal
//! transcrit n'est pas une faute visible que l'utilisateur corrige, c'est une
//! **entité en double**, créée en silence. Les noms déjà connus de la base
//! locale sont exactement le contexte qui manque au décodeur, et c'est un
//! contexte qu'aucun service distant n'a.

use rusqlite::Connection;

use crate::embedder::CoreError;

/// Fréquence d'échantillonnage attendue par whisper. Un flux capté à une autre
/// fréquence doit être ré-échantillonné par l'hôte AVANT d'arriver ici : le
/// décodeur ne le détecte pas, il transcrit du charabia.
pub const SAMPLE_RATE_HZ: u32 = 16_000;

/// Budget de tokens de l'amorçage. whisper.cpp ne garde que `n_text_ctx / 2`
/// tokens de prompt (224 sur tous les modèles publiés), et la troncature
/// conserve la **fin** de la liste. On reste franchement en dessous : tant que
/// c'est le cas, l'ordre d'émission n'a aucune conséquence. Relever ce budget
/// au-delà de 224 obligerait à émettre les noms les plus forts en dernier.
pub const PRIME_TOKEN_BUDGET: usize = 180;

/// Types d'entités dont le nom est un nom propre à coup sûr. La liste ne suffit
/// PAS à elle seule : le vocabulaire des types est extensible par l'usage
/// (`brand`, `device`, `restaurant` sont apparus après coup), donc une liste
/// figée reléguerait au second rideau des noms comme Nuphy ou Kodawari Ramen,
/// c'est-à-dire exactement ceux que le décodeur écorche. Elle est doublée d'un
/// test sur la CAPITALE initiale, qui lui, ne dérive pas avec le vocabulaire.
///
/// Les deux ensemble se rattrapent l'un l'autre : la capitale attrape les
/// marques et les lieux quel que soit leur type, le type attrape les noms
/// propres écrits en bas de casse (`oaio`, `sinam`).
const PROPER_NOUN_TYPES: &[&str] = &["person", "place", "organization", "project", "animal"];

/// Au-delà, ce n'est plus un nom mais une phrase : ça mange le budget de
/// plusieurs vrais noms pour un gain nul.
const MAX_NAME_CHARS: usize = 48;

#[derive(Debug, Clone)]
pub struct PrimeOptions {
    /// Budget de tokens du prompt final.
    pub budget_tokens: usize,
    /// Garde-fou de lecture : le budget tranche bien avant, mais une base de
    /// 50 000 entités ne doit pas être lue en entier à chaque capture.
    pub max_names: usize,
    /// Inclure les alias connus d'une entité (« Théo » pour « Théo Marchand »).
    pub include_aliases: bool,
}

impl Default for PrimeOptions {
    fn default() -> Self {
        Self { budget_tokens: PRIME_TOKEN_BUDGET, max_names: 200, include_aliases: true }
    }
}

/// Les noms connus, dans l'ordre où ils méritent la place du prompt.
///
/// Le classement est `memory_strength` d'abord, et c'est un choix : cette
/// colonne porte déjà la décroissance d'Ebbinghaus, donc « le plus fort » veut
/// dire « mentionné souvent ET récemment », ce qui est la meilleure prédiction
/// disponible de ce que la prochaine capture va nommer. `mention_count`
/// départage, le nom canonique rend l'ordre déterministe (donc testable).
///
/// Passent devant les noms qui ressemblent à des noms propres : capitale
/// initiale, ou type qui en garantit un. Les mots communs (`sérendipité`, un
/// outil au nom générique) attendent qu'il reste du budget, parce que le
/// décodeur les écrit déjà bien sans aide.
///
/// Les entités fusionnées, archivées ou en attente de validation sont exclues :
/// amorcer avec un nom que le produit a déjà retiré du graphe, c'est pousser le
/// décodeur à le réécrire.
pub fn graph_names(conn: &Connection, opts: &PrimeOptions) -> Result<Vec<String>, CoreError> {
    let mut stmt = conn.prepare(
        "SELECT canonical_name, aliases, COALESCE(type, ''), \
                COALESCE(memory_strength, 0.0), COALESCE(mention_count, 0) \
         FROM entities \
         WHERE COALESCE(status, 'active') = 'active' \
           AND archived_at IS NULL \
           AND merged_into_id IS NULL \
           AND canonical_name IS NOT NULL \
           AND TRIM(canonical_name) <> '' \
         ORDER BY COALESCE(memory_strength, 0.0) DESC, \
                  COALESCE(mention_count, 0) DESC, \
                  canonical_name ASC \
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([opts.max_names as i64], |r| {
        Ok(NameCandidate {
            name: r.get(0)?,
            // `aliases` est un tableau JSON écrit par l'hôte ; un contenu
            // illisible n'est pas une erreur, on garde le nom canonique.
            aliases: r
                .get::<_, Option<String>>(1)?
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                .and_then(|v| v.as_array().cloned())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| i.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            kind: r.get(2)?,
            strength: r.get(3)?,
            mentions: r.get(4)?,
        })
    })?;
    let mut candidates: Vec<NameCandidate> = Vec::new();
    for row in rows {
        candidates.push(row?);
    }
    Ok(rank_names(candidates, opts.include_aliases))
}

/// Une entité candidate à l'amorçage, telle que n'importe quel hôte peut la
/// décrire. Elle existe pour que le classement ait UNE implémentation : le
/// cœur la lit de sa propre base, l'app mobile la lit de son réplica local, et
/// les deux passent par `rank_names`. Un téléphone client n'a pas de base du
/// cœur, mais il a le réplica du graphe : sans ce point d'entrée, il aurait
/// fallu recopier la règle côté app, où elle aurait dérivé.
#[derive(Debug, Clone)]
pub struct NameCandidate {
    pub name: String,
    pub aliases: Vec<String>,
    /// Type d'entité, vide si inconnu.
    pub kind: String,
    pub strength: f64,
    pub mentions: i64,
}

/// Le classement, et lui seul.
///
/// `memory_strength` d'abord, et c'est un choix : cette valeur porte déjà la
/// décroissance d'Ebbinghaus, donc « le plus fort » veut dire « mentionné
/// souvent ET récemment », ce qui est la meilleure prédiction disponible de ce
/// que la prochaine capture va nommer. Le nombre de mentions départage, le nom
/// rend l'ordre déterministe, donc testable.
///
/// Passent devant ceux qui ressemblent à des noms propres : capitale initiale
/// **ou** type qui en garantit un. Les deux critères se rattrapent l'un
/// l'autre, et il en faut deux : le vocabulaire des types est extensible par
/// l'usage (`brand`, `device`, `restaurant` sont apparus après coup), donc une
/// liste figée reléguerait au second rideau des noms comme Nuphy ou Kodawari
/// Ramen, c'est-à-dire exactement ceux que le décodeur écorche ; et la capitale
/// seule raterait les noms propres écrits en bas de casse (`oaio`, `sinam`).
pub fn rank_names(mut candidates: Vec<NameCandidate>, include_aliases: bool) -> Vec<String> {
    candidates.sort_by(|a, b| {
        proper_rank(a)
            .cmp(&proper_rank(b))
            .then(b.strength.total_cmp(&a.strength))
            .then(b.mentions.cmp(&a.mentions))
            .then(a.name.cmp(&b.name))
    });

    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let push = |name: &str, out: &mut Vec<String>, seen: &mut Vec<String>| {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return;
        }
        let key = name.to_lowercase();
        if seen.contains(&key) {
            return;
        }
        seen.push(key);
        out.push(name.to_string());
    };
    for candidate in &candidates {
        push(&candidate.name, &mut out, &mut seen);
        if include_aliases {
            for alias in &candidate.aliases {
                push(alias, &mut out, &mut seen);
            }
        }
    }
    out
}

/// 0 pour un nom propre, 1 pour le reste.
fn proper_rank(c: &NameCandidate) -> u8 {
    let capitale = c
        .name
        .trim()
        .chars()
        .next()
        .is_some_and(|ch| ch.is_uppercase());
    if capitale || PROPER_NOUN_TYPES.contains(&c.kind.as_str()) {
        0
    } else {
        1
    }
}

/// Estimation du coût en tokens d'un nom, sans tokenizer.
///
/// Le vrai tokenizer vit dans le modèle, donc il n'est disponible que sous la
/// feature `voice` (`Transcriber::fit_prompt` s'en sert et tranche au token
/// près). Ici on majore volontairement : le BPE de whisper découpe un nom
/// propre inconnu en morceaux courts, 3 caractères par token est la borne
/// prudente, plus un token pour le séparateur.
pub fn estimate_tokens(name: &str) -> usize {
    name.chars().count().div_ceil(3) + 1
}

/// Coupe la liste au budget, en gardant l'ordre (donc les plus forts).
pub fn fit_names(names: &[String], budget_tokens: usize) -> Vec<String> {
    let mut spent = 0usize;
    let mut kept = Vec::new();
    for name in names {
        let cost = estimate_tokens(name);
        if spent + cost > budget_tokens {
            continue;
        }
        spent += cost;
        kept.push(name.clone());
    }
    kept
}

/// Rend la liste sous la forme que whisper attend : du texte, pas une
/// structure. Le point final compte, il ferme la phrase de contexte au lieu de
/// laisser le décodeur croire qu'il doit continuer l'énumération.
pub fn join_names(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    format!("{}.", names.join(", "))
}

/// L'amorçage complet, prêt à passer au décodeur. Vide si la base ne connaît
/// encore personne, et c'est le bon comportement : un prompt vide vaut mieux
/// qu'un prompt inventé.
pub fn graph_prompt(conn: &Connection, opts: &PrimeOptions) -> Result<String, CoreError> {
    let names = graph_names(conn, opts)?;
    Ok(join_names(&fit_names(&names, opts.budget_tokens)))
}

/// Conversion PCM 16 bits signé vers les flottants attendus par le décodeur.
/// C'est le format que rendent `AudioRecord` (Android) et `AVAudioRecorder`
/// (iOS), donc la conversion appartient au core et pas à chaque hôte.
pub fn pcm16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|s| *s as f32 / 32768.0).collect()
}

/// La même chose depuis les OCTETS bruts, petit-boutiste, tels que
/// `AudioRecord.read(byte[])` les rend.
///
/// C'est la forme qui passe la frontière FFI : un tableau d'octets y voyage
/// tel quel, alors qu'un tableau d'entiers 16 bits devient une liste
/// d'objets, soit 320 000 allocations pour vingt secondes de parole. Un octet
/// isolé en fin de tampon est ignoré : c'est une trame coupée, pas une erreur.
pub fn pcm16le_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]) as f32 / 32768.0)
        .collect()
}

// ── Décodeur ────────────────────────────────────────────────────────────────

#[cfg(feature = "voice")]
pub use decoder::{Segment, Transcriber, TranscribeOptions, Transcript, SpeechGuard};

#[cfg(feature = "voice")]
mod decoder {
    use super::*;
    use whisper_rs::{
        FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext,
        WhisperVadContextParams, WhisperVadParams,
    };

    /// Ce qui écarte une capture inventée.
    ///
    /// ⚠️ **Les deux signaux du décodeur ne séparent rien, c'est mesuré.** Le
    /// premier réflexe est d'écarter un segment quand `no_speech_prob` est
    /// haute ET `avg_logprob` basse, comme le fait whisper.cpp en interne. Sur
    /// les modèles publiés (base, small et large-v3-turbo quantifiés),
    /// `no_speech_prob` vaut **0 partout**, y compris sur du souffle de pièce
    /// sans un mot : le jeton `<|nospeech|>` n'y est pas exploitable. Et la
    /// logprob ne tranche pas non plus : souffle pur à -0,305 contre parole
    /// réelle à -0,258, les deux plages se recouvrent. Une règle bâtie sur eux
    /// est INERTE, et pire qu'absente puisqu'elle rassure.
    ///
    /// Ce qui marche, dans l'ordre :
    ///
    /// 1. le **détecteur de parole** ([`TranscribeOptions::vad_model_path`]) :
    ///    le décodeur ne voit que ce qui est de la parole, donc il n'a rien à
    ///    inventer. C'est la vraie porte. ⚠️ Il est appliqué ICI, à la main :
    ///    whisper.cpp ne fait le filtrage que dans `whisper_full()`, jamais
    ///    dans `whisper_full_with_state()` par lequel passe toute liaison qui
    ///    gère son propre état. Poser le drapeau et le chemin du modèle ne
    ///    produit alors **aucun effet et aucune erreur**, pas même avec un
    ///    chemin inexistant ;
    /// 2. la **forme du texte** : sur un blanc, whisper ne rend pas une phrase
    ///    plausible, il rend une annotation de bruit (`*soupir*`, `[Musique]`,
    ///    `(rires)`, `♪`). Ce n'est pas de la parole, ça ne doit pas devenir une
    ///    note.
    ///
    /// Les deux nombres restent exposés sur [`Segment`] : ils servent au
    /// diagnostic, pas à décider.
    #[derive(Debug, Clone)]
    pub struct SpeechGuard {
        /// Écarter les segments qui sont une annotation de bruit, pas de la
        /// parole.
        pub drop_sound_events: bool,
    }

    impl Default for SpeechGuard {
        fn default() -> Self {
            Self { drop_sound_events: true }
        }
    }

    /// Un segment entièrement enfermé dans une notation d'événement sonore.
    /// whisper les rend avec des marqueurs stables, quelle que soit la langue.
    fn is_sound_event(text: &str) -> bool {
        let t = text.trim();
        if t.is_empty() {
            return false;
        }
        // Le cas des points de suspension se teste AVANT de retirer la
        // ponctuation finale, sinon il ne reste rien à tester.
        if t.chars().all(|c| c == '.' || c == '…' || c.is_whitespace()) {
            return true;
        }
        let t = t.trim_end_matches(['.', '!', '?']).trim();
        const PAIRS: &[(char, char)] = &[('*', '*'), ('[', ']'), ('(', ')'), ('♪', '♪')];
        PAIRS.iter().any(|(open, close)| {
            t.starts_with(*open) && t.ends_with(*close) && t.chars().count() > 1
        })
    }

    #[derive(Debug, Clone, Default)]
    pub struct TranscribeOptions {
        /// Code ISO 639-1 ; `None` laisse whisper détecter la langue.
        pub language: Option<String>,
        /// Amorçage textuel, typiquement le retour de [`graph_prompt`].
        pub initial_prompt: Option<String>,
        /// Chemin du modèle de détection de parole (silero, environ 1 Mo).
        /// Quand il est fourni, le décodeur ne voit que les passages parlés :
        /// c'est ce qui l'empêche d'inventer sur un blanc, et ça coûte moins
        /// cher que de décoder du silence.
        pub vad_model_path: Option<String>,
        pub guard: SpeechGuard,
    }

    #[derive(Debug, Clone)]
    pub struct Segment {
        pub text: String,
        pub start_ms: i64,
        pub end_ms: i64,
        pub no_speech_prob: f32,
        pub avg_logprob: f32,
        /// Rejeté par le garde-fou : gardé dans la sortie, jamais dans le texte.
        pub dropped: bool,
    }

    #[derive(Debug, Clone)]
    pub struct Transcript {
        /// Les segments retenus, recollés. C'est le texte de la capture.
        pub text: String,
        pub segments: Vec<Segment>,
        /// Vrai dès qu'un segment a été rejeté. L'hôte s'en sert pour envoyer
        /// la capture en « À valider » au lieu de l'écrire directement : une
        /// capture dont une partie est peut-être inventée n'entre pas telle
        /// quelle dans une mémoire personnelle.
        pub needs_review: bool,
        /// Langue détectée par le modèle (ISO 639-1), quand il en rend une.
        pub language: Option<String>,
        /// Faux quand le détecteur de parole n'a rien trouvé. L'hôte n'écrit
        /// alors RIEN : une capture vide vaut mieux qu'une capture inventée, et
        /// c'est la seule réponse honnête à un enregistrement sans parole.
        pub speech_detected: bool,
    }

    /// Décodeur whisper.cpp chargé une fois, réutilisable.
    ///
    /// Le contexte porte les poids (lecture seule) ; chaque transcription crée
    /// son propre état, donc `&self` suffit et deux captures peuvent se suivre
    /// sans recharger le modèle.
    pub struct Transcriber {
        ctx: WhisperContext,
        threads: i32,
    }

    impl Transcriber {
        /// `model_path` pointe un fichier ggml/gguf whisper (`ggml-small.bin`,
        /// `ggml-large-v3-turbo-q5_0.bin`...). Fichier de DONNÉES, jamais
        /// commité, jamais embarqué dans le crate.
        pub fn new(model_path: &str) -> Result<Self, CoreError> {
            let ctx = WhisperContext::new_with_params(
                model_path,
                WhisperContextParameters::default(),
            )
            .map_err(|e| CoreError::ModelLoad(format!("whisper model {model_path}: {e}")))?;
            Ok(Self { ctx, threads: 4 })
        }

        /// Sur mobile on reste bas : le décodeur partage le processeur avec le
        /// reste de l'application.
        pub fn with_threads(mut self, threads: i32) -> Self {
            self.threads = threads.max(1);
            self
        }

        /// Coupe une liste de noms au budget, au token près cette fois : le
        /// tokenizer du modèle chargé remplace l'estimation de `fit_names`.
        /// On retire par la fin, donc les noms les plus forts survivent.
        pub fn fit_prompt(&self, names: &[String], budget_tokens: usize) -> String {
            let mut kept: Vec<String> = names.to_vec();
            while !kept.is_empty() {
                let candidate = join_names(&kept);
                match self.ctx.tokenize(&candidate, budget_tokens + 1) {
                    Ok(tokens) if tokens.len() <= budget_tokens => return candidate,
                    // La seule erreur possible ici est « ça ne tient pas dans
                    // n_max_tokens », qui est précisément le cas à traiter.
                    _ => {
                        kept.pop();
                    }
                }
            }
            String::new()
        }

        /// `pcm` est du mono flottant à [`SAMPLE_RATE_HZ`].
        pub fn transcribe(
            &self,
            pcm: &[f32],
            opts: &TranscribeOptions,
        ) -> Result<Transcript, CoreError> {
            let mut state = self
                .ctx
                .create_state()
                .map_err(|e| CoreError::Transcription(format!("state: {e}")))?;

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_n_threads(self.threads);
            // Pas de repli en température : quand un segment déplaît à
            // whisper.cpp (entropie ou logprob sous les seuils), il le redécode
            // jusqu'à cinq fois de suite. Sur un téléphone c'est le pire des
            // deux mondes, on paie plusieurs passes pour un texte à peine
            // différent, et la latence devient imprévisible.
            params.set_temperature_inc(0.0);
            params.set_translate(false);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            // ⚠️ Ne JAMAIS laisser ce champ au défaut de whisper.cpp : il vaut
            // `"en"`, pas « détecte ». Une capture française décodée sous le
            // jeton anglais n'est pas mal transcrite, elle est TRADUITE
            // (« Acheter des piles, du café » ressortait en « Buy batteries,
            // coffee »), et rien dans la sortie ne signale que ça s'est
            // produit. `"auto"` déclenche la détection sur l'audio.
            params.set_language(Some(opts.language.as_deref().unwrap_or("auto")));
            if let Some(prompt) = opts.initial_prompt.as_deref() {
                // set_initial_prompt panique sur un octet nul, et un nom
                // d'entité vient d'une base que l'utilisateur remplit.
                let clean = prompt.replace('\0', " ");
                if !clean.trim().is_empty() {
                    params.set_initial_prompt(&clean);
                }
            }

            // Filtrage de la parole en amont, jamais par le drapeau de
            // whisper.cpp : voir la note sur `SpeechGuard`.
            let parole;
            let entree: &[f32] = match opts.vad_model_path.as_deref() {
                Some(path) => {
                    parole = speech_only(path, pcm)?;
                    if parole.is_empty() {
                        return Ok(Transcript {
                            text: String::new(),
                            segments: Vec::new(),
                            needs_review: false,
                            language: None,
                            speech_detected: false,
                        });
                    }
                    &parole
                }
                None => pcm,
            };

            // whisper encode TOUJOURS une fenêtre de 30 s : une capture de 6 s
            // est complétée par du vide, et l'encodeur paie le vide plein pot.
            // Réduire le contexte audio à la durée réelle est le plus gros
            // levier de vitesse sur un téléphone, et il est gratuit en qualité
            // tant qu'on garde une marge (mesuré : rien ne bouge dans le texte).
            // 1500 trames = 30 s.
            let seconds = entree.len() as f32 / SAMPLE_RATE_HZ as f32;
            let ctx = ((seconds / 30.0 * 1500.0).ceil() as i32 + 128).clamp(256, 1500);
            params.set_audio_ctx(ctx);

            state
                .full(params, entree)
                .map_err(|e| CoreError::Transcription(format!("decode: {e}")))?;

            let mut segments = Vec::new();
            let mut kept_text = String::new();
            let mut needs_review = false;
            let count = state.full_n_segments();
            for i in 0..count {
                let seg = match state.get_segment(i) {
                    Some(s) => s,
                    None => continue,
                };
                let text = seg.to_str_lossy().unwrap_or_default().to_string();
                let no_speech = seg.no_speech_probability();
                let avg_logprob = mean_logprob(&seg);
                let dropped = opts.guard.drop_sound_events && is_sound_event(&text);
                if dropped {
                    needs_review = true;
                } else if !text.trim().is_empty() {
                    if !kept_text.is_empty() {
                        kept_text.push(' ');
                    }
                    kept_text.push_str(text.trim());
                }
                segments.push(Segment {
                    text,
                    start_ms: seg.start_timestamp() * 10,
                    end_ms: seg.end_timestamp() * 10,
                    no_speech_prob: no_speech,
                    avg_logprob,
                    dropped,
                });
            }

            let language = whisper_rs::get_lang_str(state.full_lang_id_from_state())
                .map(|s| s.to_string());

            Ok(Transcript {
                text: kept_text,
                segments,
                needs_review,
                language,
                speech_detected: true,
            })
        }
    }

    /// Ne garde que les passages parlés, recollés. Rendre un vecteur vide veut
    /// dire « personne n'a parlé », ce qui est une réponse et pas un échec.
    fn speech_only(model_path: &str, pcm: &[f32]) -> Result<Vec<f32>, CoreError> {
        let mut vad = WhisperVadContext::new(model_path, WhisperVadContextParams::new())
            .map_err(|e| CoreError::ModelLoad(format!("vad {model_path}: {e}")))?;
        let segments = vad
            .segments_from_samples(WhisperVadParams::new(), pcm)
            .map_err(|e| CoreError::Transcription(format!("vad: {e}")))?;
        let mut out = Vec::new();
        for segment in segments {
            // Les bornes du VAD sont en centisecondes.
            let debut = (segment.start * 0.01 * SAMPLE_RATE_HZ as f32).max(0.0) as usize;
            let fin = ((segment.end * 0.01 * SAMPLE_RATE_HZ as f32) as usize).min(pcm.len());
            if debut < fin {
                out.extend_from_slice(&pcm[debut..fin]);
            }
        }
        Ok(out)
    }

    /// Moyenne des log-probabilités des tokens du segment. whisper.cpp ne rend
    /// pas la valeur agrégée, seulement le `plog` de chaque token.
    fn mean_logprob(seg: &whisper_rs::WhisperSegment<'_>) -> f32 {
        let n = seg.n_tokens();
        if n <= 0 {
            return 0.0;
        }
        let mut sum = 0.0f32;
        let mut counted = 0f32;
        for t in 0..n {
            if let Some(token) = seg.get_token(t) {
                sum += token.token_data().plog;
                counted += 1.0;
            }
        }
        if counted == 0.0 {
            0.0
        } else {
            sum / counted
        }
    }

    #[cfg(test)]
    mod tests {
        use super::is_sound_event;

        #[test]
        fn une_annotation_de_bruit_n_est_pas_une_capture() {
            for bruit in ["*soupir*", "[Musique]", "(rires)", "♪♪", "...", " *bruit de porte* "] {
                assert!(is_sound_event(bruit), "{bruit} devrait être écarté");
            }
        }

        #[test]
        fn une_phrase_reste_une_phrase() {
            for parole in [
                "Notez que Romain part en vacances.",
                "Appeler Théo (le voisin) demain.",
                "L'écran arrive jeudi",
                "",
            ] {
                assert!(!is_sound_event(parole), "{parole} ne devrait pas être écarté");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE entities (
                id TEXT PRIMARY KEY, type TEXT, canonical_name TEXT NOT NULL,
                aliases TEXT DEFAULT '[]', mention_count INTEGER DEFAULT 1,
                memory_strength REAL DEFAULT 1.0, status TEXT DEFAULT 'active',
                archived_at TIMESTAMP, merged_into_id TEXT)",
        )
        .unwrap();
        conn
    }

    fn add(conn: &Connection, id: &str, kind: &str, name: &str, strength: f64) {
        conn.execute(
            "INSERT INTO entities (id, type, canonical_name, memory_strength) \
             VALUES (?1, ?2, ?3, ?4)",
            params![id, kind, name, strength],
        )
        .unwrap();
    }

    #[test]
    fn les_noms_propres_passent_devant_les_mots_communs() {
        let conn = db();
        // Le concept est plus fort, et il passe quand même après : ce n'est pas
        // lui que le décodeur risque d'écorcher.
        add(&conn, "e1", "concept", "sérendipité", 1.0);
        add(&conn, "e2", "person", "Théo Marchand", 0.2);
        let names = graph_names(&conn, &PrimeOptions::default()).unwrap();
        assert_eq!(names, vec!["Théo Marchand".to_string(), "sérendipité".to_string()]);
    }

    /// Le vocabulaire des types grandit avec l'usage : une marque ou un
    /// restaurant n'était pas prévu par la liste, et ce sont pourtant des noms
    /// que le décodeur invente. La capitale les rattrape.
    #[test]
    fn une_capitale_vaut_un_type_porteur_de_nom_propre() {
        let conn = db();
        add(&conn, "e1", "tool", "obsidian vault", 1.0);
        add(&conn, "e2", "brand", "Nuphy", 0.3);
        add(&conn, "e3", "restaurant", "Kodawari Ramen", 0.2);
        let names = graph_names(&conn, &PrimeOptions::default()).unwrap();
        assert_eq!(names, vec!["Nuphy", "Kodawari Ramen", "obsidian vault"]);
    }

    /// Et le type rattrape ce que la capitale rate : une marque écrite en bas
    /// de casse reste un nom propre.
    #[test]
    fn un_nom_propre_en_bas_de_casse_reste_devant_par_son_type() {
        let conn = db();
        add(&conn, "e1", "tool", "gestionnaire de mots de passe", 1.0);
        add(&conn, "e2", "organization", "oaio", 0.2);
        let names = graph_names(&conn, &PrimeOptions::default()).unwrap();
        assert_eq!(names, vec!["oaio", "gestionnaire de mots de passe"]);
    }

    #[test]
    fn le_plus_fort_passe_devant_a_type_egal() {
        let conn = db();
        add(&conn, "e1", "person", "Clara", 0.1);
        add(&conn, "e2", "person", "Marie", 0.9);
        add(&conn, "e3", "person", "Yanis", 0.5);
        let names = graph_names(&conn, &PrimeOptions::default()).unwrap();
        assert_eq!(names, vec!["Marie", "Yanis", "Clara"]);
    }

    #[test]
    fn une_entite_retiree_du_graphe_n_amorce_plus_rien() {
        let conn = db();
        add(&conn, "e1", "person", "Vivante", 1.0);
        add(&conn, "e2", "person", "Archivée", 1.0);
        conn.execute("UPDATE entities SET archived_at = '2026-01-01' WHERE id = 'e2'", [])
            .unwrap();
        add(&conn, "e3", "person", "Fusionnée", 1.0);
        conn.execute("UPDATE entities SET merged_into_id = 'e1' WHERE id = 'e3'", [])
            .unwrap();
        add(&conn, "e4", "person", "Proposée", 1.0);
        conn.execute("UPDATE entities SET status = 'pending' WHERE id = 'e4'", []).unwrap();
        assert_eq!(graph_names(&conn, &PrimeOptions::default()).unwrap(), vec!["Vivante"]);
    }

    #[test]
    fn les_alias_entrent_derriere_leur_nom_canonique_et_sans_doublon() {
        let conn = db();
        add(&conn, "e1", "person", "Théo Marchand", 1.0);
        conn.execute(
            "UPDATE entities SET aliases = ?1 WHERE id = 'e1'",
            params![r#"["Théo", "théo marchand", ""]"#],
        )
        .unwrap();
        let names = graph_names(&conn, &PrimeOptions::default()).unwrap();
        assert_eq!(names, vec!["Théo Marchand".to_string(), "Théo".to_string()]);

        let sans = PrimeOptions { include_aliases: false, ..PrimeOptions::default() };
        assert_eq!(graph_names(&conn, &sans).unwrap(), vec!["Théo Marchand"]);
    }

    #[test]
    fn un_alias_illisible_ne_fait_pas_perdre_l_entite() {
        let conn = db();
        add(&conn, "e1", "person", "Marie", 1.0);
        conn.execute("UPDATE entities SET aliases = 'pas du json' WHERE id = 'e1'", [])
            .unwrap();
        assert_eq!(graph_names(&conn, &PrimeOptions::default()).unwrap(), vec!["Marie"]);
    }

    #[test]
    fn une_phrase_deguisee_en_nom_ne_mange_pas_le_budget() {
        let conn = db();
        let long = "a".repeat(MAX_NAME_CHARS + 1);
        add(&conn, "e1", "person", &long, 1.0);
        add(&conn, "e2", "person", "Marie", 0.5);
        assert_eq!(graph_names(&conn, &PrimeOptions::default()).unwrap(), vec!["Marie"]);
    }

    #[test]
    fn le_budget_coupe_la_liste_sans_la_desordonner() {
        let names: Vec<String> =
            (0..50).map(|i| format!("Personne{i:02}")).collect();
        let kept = fit_names(&names, 20);
        assert!(!kept.is_empty() && kept.len() < names.len());
        assert_eq!(kept[0], names[0]);
        let spent: usize = kept.iter().map(|n| estimate_tokens(n)).sum();
        assert!(spent <= 20, "budget dépassé : {spent}");
    }

    #[test]
    fn un_graphe_vide_donne_un_amorcage_vide() {
        let conn = db();
        assert_eq!(graph_prompt(&conn, &PrimeOptions::default()).unwrap(), "");
    }

    #[test]
    fn l_amorcage_est_du_texte_ferme() {
        let conn = db();
        add(&conn, "e1", "person", "Marie", 1.0);
        add(&conn, "e2", "place", "Lyon", 0.5);
        assert_eq!(graph_prompt(&conn, &PrimeOptions::default()).unwrap(), "Marie, Lyon.");
    }

    #[test]
    fn les_octets_bruts_donnent_les_memes_echantillons() {
        let samples: [i16; 4] = [0, 1234, -1234, i16::MIN];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        assert_eq!(pcm16le_to_f32(&bytes), pcm16_to_f32(&samples));
        // Une trame coupée en deux ne fait pas échouer la capture.
        let mut tronque = bytes.clone();
        tronque.push(7);
        assert_eq!(pcm16le_to_f32(&tronque).len(), samples.len());
    }

    #[test]
    fn le_pcm_16_bits_arrive_borne() {
        let out = pcm16_to_f32(&[0, i16::MAX, i16::MIN]);
        assert_eq!(out[0], 0.0);
        assert!((out[1] - 0.999_97).abs() < 1e-4);
        assert_eq!(out[2], -1.0);
    }
}
