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

/// Types d'entités dont le nom est un nom propre, donc ceux que le décodeur
/// n'a aucune chance de deviner. Les autres (`concept`, `tool`) sont le plus
/// souvent des mots communs que le modèle écrit déjà bien : ils passent après,
/// et seulement s'il reste du budget.
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
/// Les entités fusionnées, archivées ou en attente de validation sont exclues :
/// amorcer avec un nom que le produit a déjà retiré du graphe, c'est pousser le
/// décodeur à le réécrire.
pub fn graph_names(conn: &Connection, opts: &PrimeOptions) -> Result<Vec<String>, CoreError> {
    // Une seule source pour la liste des types : la constante ci-dessus.
    let proper: String = PROPER_NOUN_TYPES
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = conn.prepare(&format!(
        "SELECT canonical_name, aliases, \
                CASE WHEN COALESCE(type, '') IN ({proper}) \
                     THEN 0 ELSE 1 END AS name_rank \
         FROM entities \
         WHERE COALESCE(status, 'active') = 'active' \
           AND archived_at IS NULL \
           AND merged_into_id IS NULL \
           AND canonical_name IS NOT NULL \
           AND TRIM(canonical_name) <> '' \
         ORDER BY name_rank ASC, \
                  COALESCE(memory_strength, 0.0) DESC, \
                  COALESCE(mention_count, 0) DESC, \
                  canonical_name ASC \
         LIMIT ?1"
    ))?;
    let rows = stmt.query_map([opts.max_names as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;

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

    for row in rows {
        let (canonical, aliases) = row?;
        push(&canonical, &mut out, &mut seen);
        if !opts.include_aliases {
            continue;
        }
        // `aliases` est un tableau JSON écrit par l'hôte ; un contenu illisible
        // n'est pas une erreur de transcription, on garde le nom canonique.
        if let Some(raw) = aliases {
            if let Ok(serde_json::Value::Array(items)) = serde_json::from_str(&raw) {
                for item in items {
                    if let Some(alias) = item.as_str() {
                        push(alias, &mut out, &mut seen);
                    }
                }
            }
        }
    }
    Ok(out)
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

// ── Décodeur ────────────────────────────────────────────────────────────────

#[cfg(feature = "voice")]
pub use decoder::{Segment, Transcriber, TranscribeOptions, Transcript, SpeechGuard};

#[cfg(feature = "voice")]
mod decoder {
    use super::*;
    use whisper_rs::{
        FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
    };

    /// Seuils de rejet des segments hallucinés.
    ///
    /// whisper invente du texte plausible sur un silence ou un passage
    /// inaudible. Les deux signaux ne suffisent qu'ENSEMBLE : une probabilité
    /// de silence élevée seule condamne des phrases murmurées mais réelles, une
    /// logprob basse seule condamne les noms propres, c'est-à-dire exactement
    /// ce qu'on cherche à garder. Ce sont les seuils par défaut de whisper.cpp,
    /// appliqués ici au niveau du segment plutôt qu'au décodage.
    #[derive(Debug, Clone)]
    pub struct SpeechGuard {
        pub no_speech_max: f32,
        pub logprob_min: f32,
    }

    impl Default for SpeechGuard {
        fn default() -> Self {
            Self { no_speech_max: 0.6, logprob_min: -1.0 }
        }
    }

    #[derive(Debug, Clone, Default)]
    pub struct TranscribeOptions {
        /// Code ISO 639-1 ; `None` laisse whisper détecter la langue.
        pub language: Option<String>,
        /// Amorçage textuel, typiquement le retour de [`graph_prompt`].
        pub initial_prompt: Option<String>,
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
            params.set_translate(false);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            if let Some(lang) = opts.language.as_deref() {
                params.set_language(Some(lang));
            }
            if let Some(prompt) = opts.initial_prompt.as_deref() {
                // set_initial_prompt panique sur un octet nul, et un nom
                // d'entité vient d'une base que l'utilisateur remplit.
                let clean = prompt.replace('\0', " ");
                if !clean.trim().is_empty() {
                    params.set_initial_prompt(&clean);
                }
            }

            state
                .full(params, pcm)
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
                let dropped = no_speech > opts.guard.no_speech_max
                    && avg_logprob < opts.guard.logprob_min;
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

            Ok(Transcript { text: kept_text, segments, needs_review, language })
        }
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
    fn le_pcm_16_bits_arrive_borne() {
        let out = pcm16_to_f32(&[0, i16::MAX, i16::MIN]);
        assert_eq!(out[0], 0.0);
        assert!((out[1] - 0.999_97).abs() < 1e-4);
        assert_eq!(out[2], -1.0);
    }
}
