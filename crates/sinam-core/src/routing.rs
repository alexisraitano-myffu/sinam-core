//! Deterministic Dream Cycle routing (SYN-111 / T2).
//!
//! Faithful port of the Python brain's per-capture pipeline
//! (`dream_cycle/cycle.py::_process_entry` and everything it fans out to:
//! `step2_resolve`, `compute_confidence`, `step4_route`,
//! `step5_validate_pending`, `facts_store.insert_fact`, intentions, atomic
//! notes, project entries) — LLM I/O excluded. The seam is: classified JSON
//! in, database writes out. Sub-LLM work (project synthesis, resummary,
//! resource fetch) is returned to the host as a work list, never performed
//! here.
//!
//! Parity discipline (golden-tested against the frozen Python reference):
//! - SQL casefolding stays SQL (`LOWER(...)` in the same statements); Python
//!   `str.lower()` sites use Rust `to_lowercase()`;
//! - float order of operations matches `compute_confidence` exactly;
//! - Python truthiness is reproduced where the code branched on it
//!   (`classified.get("entities")` is false for `[]`);
//! - `json.dumps(..., ensure_ascii=False)` byte layout only matters where
//!   the string feeds the embedder (`entity_embedding_text`) — reproduced by
//!   `py_dumps`; stored JSON is compared content-wise by the golden
//!   normalizer, so serde's compact form is fine there.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rusqlite::types::Value as SqlV;
use rusqlite::{params, params_from_iter, Connection};
use serde_json::{json, Map, Value};

use crate::embedder::{CoreError, Embedder};
use crate::storage::{search_entities_on, search_live_tasks_on, Storage, TaskHit};

// Same tunables and defaults as cycle.py; the env overrides keep working
// because host and core share the process environment.
const MIN_ENTITY_PERSISTENCE: f64 = 2.0;

/// Le palier exigé quand l'entité n'a RIEN d'autre pour elle : inconnue, vue
/// une seule fois, un seul fait.
///
/// La persistance mesure la nature de ce qui est affirmé, pas ce qu'on sait de
/// l'entité, donc à 2 elle laissait naître une fiche sur « Vivatech c'est le
/// 24 » — un fait de persistance 3, qui n'était que la date redite. 4 est le
/// palier que `compute_confidence` traite déjà comme durable (bonus 0,15 contre
/// 0,05 à 3), et c'est celui qu'un arbitrage antérieur a retenu comme preuve
/// suffisante à lui seul : « J'ai la fête de Pierre le 20 » porte un fait à 4 et
/// crée toujours sa fiche.
const LONE_ENTITY_PERSISTENCE: f64 = 4.0;
const MERGE_EMBEDDING_THRESHOLD_DEFAULT: f64 = 0.85;
const PROJECT_ATTACH_THRESHOLD_DEFAULT: f64 = 0.30;
const PROJECT_ATTACH_MARGIN_DEFAULT: f64 = 0.03;
const REVIEW_CONFIDENCE_THRESHOLD_DEFAULT: f64 = 0.7;

/// Ce qu'il faut pour ARCHIVER une tâche qu'une capture annule, et l'avance
/// qu'il faut sur la deuxième candidate.
///
/// Le seuil penche vers la file, à l'inverse de l'accroche d'un projet, et
/// pour une raison mesurable et pas esthétique : une tâche archivée par erreur
/// sort du backlog, et un backlog est une liste qu'on lit pour savoir ce qui
/// reste — personne n'y cherche ce qui n'y est plus. La négation d'un fait
/// peut se permettre d'agir parce que la fiche montre le trou.
const TASK_CANCEL_THRESHOLD_DEFAULT: f64 = 0.62;
const TASK_CANCEL_MARGIN_DEFAULT: f64 = 0.08;

/// SYN-190 — how close two predicate NAMES must embed to be worth proposing.
///
/// Measured 2026-08-24 on the real vocabulary, and the number is NOT the story:
/// the two distributions overlap completely, so no threshold separates them.
///
/// ```text
/// must never merge                     true synonyms
/// interviewed_at ⇄ interviewed_by 0.955   is_cousin_of ⇄ cousin_of 0.970
/// parent_of      ⇄ child_of       0.817   phone_number ⇄ phone     0.761
/// sibling_of     ⇄ cousin_of      0.765   works_as     ⇄ works_at  0.696
/// son_of         ⇄ daughter_of    0.589   family_relation ⇄ sibling_of 0.598
/// ```
///
/// `parent_of` and `child_of` are INVERSES: that merge would flip the direction
/// of the graph. So embedding a predicate name is NOT a usable general proposer,
/// and the pass below restricts it to single-valued families only — where a
/// synonym is a genuine bug (it breaks supersede) and where the relation-inverse
/// minefield does not exist, since relations have no families. Restricted that
/// way it yields 6 proposals over 91 predicates, all of them defensible.
const PREDICATE_MERGE_THRESHOLD_DEFAULT: f64 = 0.65;

/// Predicates that hold at most one live value, grouped by the claim they
/// make. A new value supersedes the previous one across the whole family:
/// `birthday` and `has_birthday` are one claim under two names, and letting
/// them sit side by side puts two contradictory dates on the same fiche.
const SINGLE_VALUED_FAMILIES: &[&[&str]] = &[
    &["works_at", "current_workplace", "employer"],
    &["lives_in", "current_city", "lives", "address"],
    &["has_birthday", "birthday", "born_on", "date_of_birth"],
    &["phone", "phone_number"],
    &["email"],
    &["age"],
    &["job_title", "current_role", "role"],
];

/// The family `predicate` belongs to, if any. Members are lowercase ASCII
/// identifiers — [`insert_fact`] interpolates them into SQL on that basis.
fn single_valued_family(predicate: &str) -> Option<&'static [&'static str]> {
    let p = predicate.trim().to_lowercase();
    SINGLE_VALUED_FAMILIES
        .iter()
        .copied()
        .find(|family| family.contains(&p.as_str()))
}

const DATE_PREDICATE_KEYWORDS: &[&str] =
    &["birthday", "birth", "date", "born", "anniversary", "anniversaire"];

/// Predicates whose date can only lie in the past: nobody is born, and no
/// anniversary is commemorated, on a day that has not happened yet. Narrower
/// than [`DATE_PREDICATE_KEYWORDS`] on purpose — a bare `date` (a deadline, a
/// next appointment) is legitimately in the future.
const PAST_ONLY_PREDICATE_KEYWORDS: &[&str] =
    &["birthday", "birth", "born", "anniversary", "anniversaire"];

/// Month names the classifier realistically emits — FR and EN, accented or
/// not, full or abbreviated. Index = month number − 1.
const MONTH_NAMES: [&[&str]; 12] = [
    &["janvier", "january", "jan"],
    &["février", "fevrier", "february", "feb", "fev"],
    &["mars", "march", "mar"],
    &["avril", "april", "apr", "avr"],
    &["mai", "may"],
    &["juin", "june", "jun"],
    &["juillet", "july", "jul", "juil"],
    &["août", "aout", "august", "aug"],
    &["septembre", "september", "sep", "sept"],
    &["octobre", "october", "oct"],
    &["novembre", "november", "nov"],
    &["décembre", "decembre", "december", "dec"],
];

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Levier de repli : remettre la quatrième clause de création en création
/// DIRECTE, sans passer par la file. Absent ou vide = comportement voulu, la
/// proposition. Écrit ici et pas côté hôte parce que c'est le core qui décide.
fn creation_directe_sans_preuve() -> bool {
    matches!(
        std::env::var("SYNAPSE_ENTITY_CREATE_UNPROVEN").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

/// Wall-clock inputs, provided by the host so the core stays deterministic
/// and testable (the Python code read the clock in place).
#[derive(Debug, Clone)]
pub struct RouteContext {
    /// ISO timestamp for `inbox.processed_at` (Python's `now` parameter).
    pub now: String,
    /// ISO date for `entities.last_mentioned` (Python: `now(utc).date()`).
    pub today: String,
    /// ISO timestamp 48h ago — expired-intentions cutoff. Kept as an opaque
    /// string: Python compared `created_at < isoformat(...)` textually.
    pub intentions_cutoff: String,
    /// `%Y-%m-%d %H:%M:%S` timestamp for note reactivation writes.
    pub now_sql: String,
}

/// One project entry persisted this capture — the host runs the live
/// synthesis (Haiku) for each, exactly like `_persist_project_entry` did
/// when given a client.
#[derive(Debug, Clone)]
pub struct ProjectSynthesis {
    pub project_id: String,
    pub entry_id: String,
    pub project_name: String,
    pub entry_content: String,
    pub entry_count: i64,
}

/// SYN-189 — what the capture's negations actually did.
///
/// Kept on the report rather than left implicit because the failure this
/// feature exists to avoid is a SILENT one: a capture saying something is no
/// longer true, and the memory doing nothing about it without ever saying so.
#[derive(Debug, Default, Clone, Copy)]
pub struct NegationOutcome {
    /// Target certain, fact obsoleted on the spot (reversible).
    pub applied: i64,
    /// Target not certain: an arbitration is waiting in the queue.
    pub proposed: i64,
    /// Nothing to negate — unknown entity, or one carrying no facts at all.
    pub unmatched: i64,
}

/// What one `obsoleted_facts` item ended up doing.
enum NegationVerdict {
    /// N live facts retired (reversibly).
    Applied(i64),
    /// Target not certain — an arbitration is queued instead.
    Proposed,
    /// Nothing on file to retire; correctly a no-op.
    Nothing,
}

#[derive(Debug, Default)]
pub struct RouteReport {
    pub entity_ids: Vec<String>,
    /// Flattened facts (with entity_canonical + source_inbox_id), the input
    /// step5 accumulates across the run.
    pub new_facts: Vec<Value>,
    pub created_note_id: Option<String>,
    pub project_syntheses: Vec<ProjectSynthesis>,
    pub fast_exit: bool,
    pub negations: NegationOutcome,
    /// Tâches retirées du backlog parce qu'une capture les annulait.
    pub cancelled_tasks: i64,
    /// Annulations qu'on n'a pas su rattacher seul : une question attend.
    pub cancellations_proposed: i64,
}

/// The routing brain: storage + (optionally) the embedder that powers the
/// merge fallback, project-attach proposals and note vectorization. Without
/// an embedder those paths degrade exactly like Python's `except Exception`
/// around `embed_text` (skip silently / leave the note unvectorized).
pub struct Brain {
    pub storage: Storage,
    embedder: Option<Arc<Embedder>>,
}

impl Brain {
    pub fn open(db_path: &str, model_dir: Option<&str>) -> Result<Self, CoreError> {
        let embedder = match model_dir {
            Some(dir) => Some(Arc::new(Embedder::new(dir)?)),
            None => None,
        };
        Self::open_shared(db_path, embedder)
    }

    /// Open sharing an already-loaded embedder (the model weighs ~235 MB and
    /// takes seconds to load; hosts opening several Brains — e.g. a test
    /// suite with one database per test — must not pay it per instance).
    pub fn open_shared(
        db_path: &str,
        embedder: Option<Arc<Embedder>>,
    ) -> Result<Self, CoreError> {
        let storage = Storage::open(db_path)?;
        Ok(Self { storage, embedder })
    }

    pub(crate) fn embed(&self, text: &str) -> Option<Vec<u8>> {
        let vec = self.embedder.as_ref()?.embed(text).ok()?;
        Some(vec.iter().flat_map(|x| x.to_le_bytes()).collect())
    }

    /// One serialized vector per ~128-token window (SYN-118): the storage
    /// keeps them all and search takes the best window per note.
    pub(crate) fn embed_chunks(&self, text: &str) -> Option<Vec<Vec<u8>>> {
        let chunks = self.embedder.as_ref()?.embed_chunks(text).ok()?;
        Some(
            chunks
                .into_iter()
                .map(|v| v.iter().flat_map(|x| x.to_le_bytes()).collect())
                .collect(),
        )
    }

    /// Chunk vectors concatenated into ONE blob (SYN-118) — the layout of the
    /// `entities`/`resources` embedding columns; scorers take the best frame.
    pub(crate) fn embed_frames(&self, text: &str) -> Option<Vec<u8>> {
        Some(self.embed_chunks(text)?.concat())
    }

    /// Embed arbitrary text with the Brain's already-loaded embedder — the
    /// host-side re-embed path after a sync apply (mirror of the backend's
    /// `embed_text`), without paying a second model load.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>, CoreError> {
        match &self.embedder {
            Some(e) => e.embed(text),
            None => Err(CoreError::Embedding(
                "brain opened without a model_dir".into(),
            )),
        }
    }

    /// Chunked variant (SYN-118) for the same re-embed path: one vector per
    /// ~128-token window, so a mobile host stores the same per-chunk rows as
    /// the desktop backend after a sync apply.
    pub fn embed_text_chunks(&self, text: &str) -> Result<Vec<Vec<f32>>, CoreError> {
        match &self.embedder {
            Some(e) => e.embed_chunks(text),
            None => Err(CoreError::Embedding(
                "brain opened without a model_dir".into(),
            )),
        }
    }

    /// Port of `_process_entry` minus classification/resources/LLM calls.
    /// `entry` = `{id, content}`; `classified` = the classifier JSON.
    /// Marks the inbox row processed. The caller handles errors by marking
    /// the entry failed (content-error policy stays host-side).
    pub fn route_capture(
        &self,
        entry: &Value,
        classified: &Value,
        ctx: &RouteContext,
    ) -> Result<RouteReport, CoreError> {
        // uuid string post-SYN-112; legacy integer ids (golden corpus,
        // pre-migration callers) are accepted as their text form.
        let capture_id: String = match entry.get("id") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return Err(CoreError::Storage("entry.id missing".into())),
        };
        let capture_id = capture_id.as_str();
        let content = entry.get("content").and_then(Value::as_str).unwrap_or("");

        let mut report = RouteReport::default();

        let is_ephemeral = truthy(classified.get("is_ephemeral"));
        let note_kind = classified
            .get("atomic_note_kind")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("note")
            .to_string();
        let atomic = classified
            .get("atomic_note")
            .and_then(Value::as_str)
            .unwrap_or("");
        let durable_note =
            !atomic.trim().is_empty() && (note_kind == "task" || note_kind == "event");

        let conn = self.storage.lock()?;

        // Pure ephemeral fast exit (no entities, no project, no durable note).
        if is_ephemeral
            && !(truthy(classified.get("entities"))
                || truthy(classified.get("project_entries"))
                || durable_note)
        {
            conn.execute_batch("BEGIN")?;
            let r = (|| -> Result<(), CoreError> {
                self.propose_project_attach_if_similar(&conn, capture_id, content, None)?;
                handle_intentions(&conn, classified, ctx)?;
                mark(&conn, capture_id, &ctx.now, "processed")?;
                Ok(())
            })();
            finish_txn(&conn, r)?;
            report.fast_exit = true;
            return Ok(report);
        }

        // Resolve (step 2) outside the transaction, like Python.
        let resolved = if truthy(classified.get("entities")) {
            Some(self.resolve(&conn, classified, ctx, content))
        } else {
            None
        };
        if let Some(resolved) = &resolved {
            for ent in resolved {
                for fact in &ent.facts {
                    let mut nf = fact.clone();
                    if let Value::Object(m) = &mut nf {
                        m.insert("entity_canonical".into(),
                                 ent.data.get("canonical_name").cloned().unwrap_or(Value::Null));
                        m.insert("source_inbox_id".into(), json!(capture_id));
                    }
                    report.new_facts.push(nf);
                }
            }
        }

        conn.execute_batch("BEGIN")?;
        let mut pending_note_vec: Option<(String, String)> = None;
        let r = (|| -> Result<(), CoreError> {
            if let Some(resolved) = &resolved {
                // Ce qui compte ici n'est pas la survie d'un ÉPHÉMÈRE — c'est le
                // sens de `durable_note`, et il reste juste pour ça — mais le
                // fait que la capture laisse une trace où accrocher une fiche.
                // Un ÉPISODE en laisse une : il asserte que quelque chose a eu
                // lieu et nourrit la frise. L'exclure faisait IGNORER
                // « Bibliothèque Forney », sans fiche et sans question, alors
                // qu'une entité nommée dans une note durable était au moins
                // proposée.
                let ancre_une_fiche = !atomic.trim().is_empty();
                report.entity_ids =
                    self.step4_route(&conn, classified, resolved, capture_id, ancre_une_fiche, ctx)?;
            }

            // SYN-189 — OUTSIDE the `resolved` guard on purpose. A capture whose
            // whole point is a negation ("Pierre ne travaille plus chez Acme")
            // may teach nothing new and come back with `entities: []`, which
            // leaves `resolved` at None. Nesting this inside would make the pure
            // negation — the very case the feature exists for — the one case it
            // never runs on.
            report.negations = self.apply_negations(&conn, classified, capture_id)?;

            // Atomic note (SYN-56/58/85 gates).
            let mut created_note_id: Option<String> = None;
            if !atomic.trim().is_empty() && (!is_ephemeral || durable_note) {
                let mut mentioned: Vec<String> = arr(classified.get("entities"))
                    .iter()
                    .filter_map(|e| e.get("canonical_name").and_then(Value::as_str))
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                for pe in arr(classified.get("project_entries")) {
                    if let Some(pc) = pe.get("project_canonical").and_then(Value::as_str) {
                        if !pc.is_empty() && !mentioned.iter().any(|m| m == pc) {
                            mentioned.push(pc.to_string());
                        }
                    }
                }
                let conf = py_float(classified.get("classification_confidence")).unwrap_or(1.0);
                let threshold = env_f64(
                    "SYNAPSE_REVIEW_CONFIDENCE_THRESHOLD",
                    REVIEW_CONFIDENCE_THRESHOLD_DEFAULT,
                );
                // SYN-182 — « À valider » covers every kind now, with a named
                // reason. The queue was built on 2026-06-29 so a doubtful TASK
                // would never be thrown away; the `episode` kind was born in
                // 2026-08 and was never added to the gate, so a model hesitating
                // at 0.2 still wrote `confirmed`. A doubtful note only clutters —
                // but an episode ASSERTS that something took place and feeds the
                // timeline, which is why it goes first.
                //
                // Recurrence is a distinct doubt from existence, and the costlier
                // one: it commits us to notifying the user every year, forever.
                // The prompt only ever justifies recurrence for a birthday, which
                // is an `event` — so recurrence on any other kind was decided
                // without a rule, and recurrence on a hesitant event IS the bare
                // anniversary case. Both go to validation; only a confident event
                // keeps its recurrence unaided, which is the named celebration the
                // prompt actually covers. Same shape as « no model-driven
                // deletion »: here, no model-driven yearly repeat.
                let recurring = truthy(classified.get("event_recurring"));
                let hesitant = conf < threshold;
                let (review_status, review_reason) =
                    if recurring && (note_kind != "event" || hesitant) {
                        ("pending", Some("recurrence_inferee"))
                    } else if hesitant {
                        match note_kind.as_str() {
                            "task" | "event" => ("pending", Some("perte_possible")),
                            _ => ("pending", Some("existence_douteuse")),
                        }
                    } else {
                        ("confirmed", None)
                    };
                // SYN-182 — reported speech gives the action to someone else. The
                // prompt has promised "never as the author's own" since SYN-85 with
                // nothing behind it: the column did not exist, so the note landed in
                // the author's backlog anyway. NULL means the author, which is also
                // every row written before today.
                let owner = classified
                    .get("atomic_note_owner")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let summary = classified
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                // SYN-119 — the classifier detects the capture language server-side.
                let language = classified
                    .get("language")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty());
                let note_id = persist_atomic_note(
                    &conn,
                    atomic.trim(),
                    summary,
                    &mentioned,
                    capture_id,
                    &note_kind,
                    // Same normalisation as the fact side: `next_occurrence`
                    // parses ISO strictly, so an event dated "12 juin" never
                    // reaches a notification at all.
                    classified
                        .get("event_date")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        // SYN-213 — la résolution d'abord, puis l'invariant :
                        // une date issue d'un jour NOMMÉ tombe sur ce jour.
                        .map(|s| {
                            let iso = resolve_date(s, &ctx.today);
                            let iso = snap_bare_day_month(&iso, content, &ctx.today);
                            snap_to_named_weekday(&iso, content, &ctx.today)
                        })
                        .as_deref(),
                    recurring,
                    review_status,
                    review_reason,
                    owner,
                    language,
                )?;
                created_note_id = Some(note_id.clone());
                let title: String = if summary.is_empty() { atomic.trim() } else { summary }
                    .chars()
                    .take(60)
                    .collect();
                pending_note_vec = Some((note_id, format!("{title}\n{}", atomic.trim())));
            }
            report.created_note_id = created_note_id.clone();

            // Project entries — N per capture, dedup by lowercased canonical.
            let mut seen_projects: HashSet<String> = HashSet::new();
            for pe in arr(classified.get("project_entries")) {
                let Some(pc) = pe.get("project_canonical").and_then(Value::as_str) else {
                    continue;
                };
                let key = pc.trim().to_lowercase();
                if key.is_empty() || seen_projects.contains(&key) {
                    continue;
                }
                seen_projects.insert(key);
                let entry_content = pe
                    .get("content")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(content)
                    .trim()
                    .to_string();
                let synthesis = persist_project_entry(
                    &conn,
                    pc.trim(),
                    &entry_content,
                    capture_id,
                    truthy(pe.get("is_new")),
                )?;
                report.project_syntheses.push(synthesis);
            }

            // Soft project-attach proposal for unrouted actionable captures.
            if seen_projects.is_empty() && (note_kind == "task" || is_ephemeral) {
                let attach_content = if !atomic.trim().is_empty() { atomic.trim() } else { content };
                self.propose_project_attach_if_similar(
                    &conn,
                    capture_id,
                    attach_content.trim(),
                    created_note_id.as_deref(),
                )?;
            }

            handle_intentions(&conn, classified, ctx)?;

            // SYN-19: a new mention reactivates the notes referencing it.
            let mentioned: Vec<String> = arr(classified.get("entities"))
                .iter()
                .filter_map(|e| e.get("canonical_name").and_then(Value::as_str))
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            crate::decay::reactivate_notes_for_entities(
                &conn,
                &mentioned,
                crate::decay::resolve_now(Some(&ctx.now_sql)),
            )?;

            // Renoncer est une DÉCISION, et une décision se garde.
            //
            // Mesuré le 2026-08-28, trois fois sur trois et sur trois captures
            // différentes : dès que `cancels_action` est rempli, le modèle
            // cesse d'écrire la note. Il traite la capture comme réglée par le
            // pointeur. Quatre formulations du prompt et deux emplacements
            // n'y ont rien changé, et la dernière disait littéralement que ce
            // champ ne décide de rien.
            //
            // Donc on ne le lui redemande pas : si une capture nomme ce
            // qu'elle annule sans rien laisser d'autre, le contenu brut EST la
            // note. `confirmed`, pas `pending` : c'est la CIBLE qui peut être
            // douteuse, jamais le fait que l'auteur a renoncé.
            let annulation = classified
                .get("cancels_action")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if annulation.is_some() && created_note_id.is_none() && !content.trim().is_empty() {
                let language = classified
                    .get("language")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty());
                let note_id = persist_atomic_note(
                    &conn,
                    content.trim(),
                    "",
                    &[],
                    capture_id,
                    "note",
                    None,
                    false,
                    "confirmed",
                    None,
                    None,
                    language,
                )?;
                created_note_id = Some(note_id.clone());
                report.created_note_id = Some(note_id);
            }

            // Une action annulée retire la tâche déjà enregistrée.
            //
            // Après la note, pas avant : la décision de renoncer EST une note,
            // et elle vient d'être écrite. On l'exclut de la recherche, sinon
            // « je ne vais finalement pas appeler le dentiste » archiverait sa
            // propre trace au lieu de la tâche de la semaine dernière.
            if let Some(action) = annulation {
                let (retirees, demandees) = self.cancel_matching_task(
                    &conn,
                    action,
                    capture_id,
                    created_note_id.as_deref(),
                )?;
                report.cancelled_tasks = retirees;
                report.cancellations_proposed = demandees;
            }

            // Une capture qui n'a RIEN laissé part en file « À valider ».
            //
            // La confiance ne pouvait pas porter ça. Elle n'est lue que dans le
            // bloc ci-dessus, gardé par `!atomic.trim().is_empty()` : quand la
            // note est nulle il n'existe aucune ligne où écrire un statut, donc
            // un modèle qui doutait à 0,2 d'un abandon était jeté aussi
            // silencieusement qu'un modèle sûr de lui. Et le prompt a raison de
            // rendre 1,0 : il note sa confiance dans le ROUTAGE, et le routage
            // d'une corvée solitaire est évident. C'est l'ABANDON qui doit être
            // relu, pas le raisonnement qui y mène.
            //
            // Rien à demander au modèle : ce qu'une capture a laissé se compte.
            // Une fiche SANS fait, sans note et sans lien ne compte pas — un nom
            // seul n'apprend rien. Le contenu brut est gardé tel quel : c'est ce
            // que l'utilisateur a écrit, et c'est la seule chose qui reste.
            let laisse_une_trace = created_note_id.is_some()
                || !report.new_facts.is_empty()
                || !seen_projects.is_empty()
                || is_ephemeral
                || !arr(classified.get("relations")).is_empty()
                || !arr(classified.get("resources")).is_empty()
                || report.negations.applied > 0
                || report.negations.proposed > 0
                || report.cancelled_tasks > 0
                || report.cancellations_proposed > 0;
            if !laisse_une_trace && !content.trim().is_empty() {
                let language = classified
                    .get("language")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty());
                let note_id = persist_atomic_note(
                    &conn,
                    content.trim(),
                    "",
                    &[],
                    capture_id,
                    "note",
                    None,
                    false,
                    "pending",
                    Some("rien_garde"),
                    None,
                    language,
                )?;
                report.created_note_id = Some(note_id);
                // Pas de vecteur : tant qu'elle est en attente, cette ligne ne
                // doit pas remonter dans une recherche comme un souvenir acquis.
            }

            mark(&conn, capture_id, &ctx.now, "processed")?;
            Ok(())
        })();
        finish_txn(&conn, r)?;
        drop(conn);

        // Post-commit, best-effort — mirrors the deferred vec flush.
        if let Some((note_id, text)) = pending_note_vec {
            if let Some(chunks) = self.embed_chunks(&text) {
                let _ = self.storage.upsert_note_vectors(&note_id, &chunks);
            }
        }

        Ok(report)
    }

    /// Report → JSON for the FFI/PyO3 boundary.
    pub fn report_to_json(report: &RouteReport) -> Value {
        json!({
            "entity_ids": report.entity_ids,
            "new_facts": report.new_facts,
            "created_note_id": report.created_note_id,
            "fast_exit": report.fast_exit,
            "cancelled_tasks": report.cancelled_tasks,
            "cancellations_proposed": report.cancellations_proposed,
            "negations": {
                "applied": report.negations.applied,
                "proposed": report.negations.proposed,
                "unmatched": report.negations.unmatched,
            },
            "project_syntheses": report.project_syntheses.iter().map(|s| json!({
                "project_id": s.project_id,
                "entry_id": s.entry_id,
                "project_name": s.project_name,
                "entry_content": s.entry_content,
                "entry_count": s.entry_count,
            })).collect::<Vec<_>>(),
        })
    }

    /// Host-facing `insert_fact` (validation endpoints, reclassify) — same
    /// dedup-reinforce + SYN-37 supersede as the routing path.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_user_fact(
        &self,
        entity_id: &str,
        predicate: &str,
        value: Value,
        confidence: f64,
        source_inbox_id: Value,
        persistence_value: i64,
        provenance_capture_id: Option<String>,
        category: Value,
    ) -> Result<String, CoreError> {
        let conn = self.storage.lock()?;
        insert_fact(
            &conn, entity_id, predicate, value, confidence, source_inbox_id,
            persistence_value, provenance_capture_id, category,
        )
    }

    /// Host-facing `_find_existing_entity` (alias-aware) → entity id.
    pub fn find_entity(
        &self,
        canonical_name: &str,
        aliases: &[String],
    ) -> Result<Option<String>, CoreError> {
        let conn = self.storage.lock()?;
        Ok(find_existing_entity(&conn, canonical_name, aliases)?
            .and_then(|row| row.get("id").and_then(Value::as_str).map(String::from)))
    }

    /// Accepter une entité proposée : elle naît, avec tout ce qui l'accompagnait.
    ///
    /// Le chemin d'écriture est celui de la création ordinaire — `upsert_entity`
    /// reste le seul endroit qui écrit une fiche, `dispatch_facts` le seul qui
    /// range un fait. Une seconde implémentation « pour l'acceptation » aurait
    /// dérivé de la première au premier changement, et personne ne l'aurait vu.
    ///
    /// Deux choses se rejouent ici et pas à la capture, parce qu'elles ont
    /// besoin de l'identifiant qui n'existait pas encore : la proposition de
    /// TYPE, transportée dans la charge, et la proposition de fusion.
    ///
    /// Idempotent : une proposition déjà tranchée ressort telle quelle.
    pub fn accept_entity_creation(
        &self,
        proposal_id: &str,
        today: &str,
    ) -> Result<Value, CoreError> {
        let conn = self.storage.lock()?;
        let Some(p) = query_row_map(
            &conn,
            "SELECT * FROM entity_creation_proposals WHERE id = ?1",
            &[SqlV::from(proposal_id.to_string())],
        )?
        else {
            return Ok(json!({"status": "not_found"}));
        };
        let statut = p.get("status").and_then(Value::as_str).unwrap_or("pending");
        if statut != "pending" {
            // Un statut à part, et pas le statut déjà en base : rendre
            // « accepted » à un second accept se lit comme une réussite côté
            // appelant, alors que rien ne s'est passé. C'est un test HTTP qui
            // l'a attrapé, la route ayant cru à un succès.
            return Ok(json!({
                "status": "already_resolved",
                "resolution": statut,
                "proposal_id": proposal_id
            }));
        }
        let raw = p.get("entity_data").and_then(Value::as_str).unwrap_or("{}");
        let mut charge: Value = serde_json::from_str(raw).unwrap_or_else(|_| json!({}));
        let facts: Vec<Value> = charge
            .get("facts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if let Value::Object(m) = &mut charge {
            m.remove("facts");
        }
        let canonical = charge
            .get("canonical_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if canonical.is_empty() {
            return Ok(json!({"status": "invalid_payload", "proposal_id": proposal_id}));
        }
        let capture_id = p
            .get("evidence_capture_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let aliases: Vec<String> = charge
            .get("aliases")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        conn.execute_batch("BEGIN")?;
        let mut sortie = json!({});
        let r = (|| -> Result<(), CoreError> {
            // Entre la question et la réponse, la même entité a pu naître par
            // une autre capture, celle-là avec une preuve. Accepter doit alors
            // enrichir la fiche existante, jamais en fabriquer une jumelle.
            let existing = find_existing_entity(&conn, &canonical, &aliases)?;
            let deja = existing.is_some();
            let statut_entite = if !deja && p.get("proposed_type").and_then(Value::as_str)
                .is_some_and(|t| !t.trim().is_empty())
            {
                "pending"
            } else {
                "active"
            };
            let id = upsert_entity(
                &conn,
                &charge,
                existing.as_ref(),
                &facts,
                &capture_id,
                statut_entite,
                today,
            )?;

            if !deja {
                if let Some(t) = p.get("proposed_type").and_then(Value::as_str)
                    .filter(|t| !t.trim().is_empty())
                {
                    conn.execute(
                        "INSERT INTO entity_type_proposals \
                         (id, proposed_type, reason, evidence_capture_id, candidate_entity_id) \
                         VALUES (?1,?2,?3,?4,?5)",
                        params![new_uuid(), t, None::<String>, &capture_id, &id],
                    )?;
                }
                let merge_type = match charge.get("type") {
                    None => Some("concept"),
                    Some(Value::Null) => None,
                    Some(v) => v.as_str(),
                };
                self.propose_merge_if_similar(&conn, &id, &canonical, merge_type, &capture_id)?;
            }

            // Même notation qu'à la capture : l'entité était neuve et vue une
            // fois, ce qui est exactement l'état dans lequel on l'avait garée.
            let scored: Vec<(Value, f64)> = facts
                .iter()
                .map(|f| {
                    let c = compute_confidence(
                        persistence_value(f),
                        f.get("evidence_strength").and_then(Value::as_str).unwrap_or("explicit"),
                        deja,
                        if deja { 2 } else { 1 },
                    );
                    (f.clone(), c)
                })
                .collect();
            dispatch_facts(&conn, Some(&id), &canonical, &scored, &capture_id)?;

            conn.execute(
                "UPDATE entity_creation_proposals SET status = 'accepted', \
                 resolved_at = CURRENT_TIMESTAMP, created_entity_id = ?2 WHERE id = ?1",
                params![proposal_id, &id],
            )?;
            sortie = json!({
                "status": "accepted",
                "proposal_id": proposal_id,
                "entity_id": id,
                "canonical_name": canonical,
                "facts_written": facts.len(),
            });
            Ok(())
        })();
        finish_txn(&conn, r)?;
        Ok(sortie)
    }

    /// Refuser : rien ne naît, et le nom n'est plus reproposé tant qu'aucune
    /// preuve n'apparaît. Une capture ultérieure qui, elle, apporte un fait
    /// durable ou un lien crée l'entité directement — le refus porte sur
    /// « nommée en passant », pas sur l'entité elle-même.
    pub fn reject_entity_creation(&self, proposal_id: &str) -> Result<Value, CoreError> {
        let conn = self.storage.lock()?;
        let Some(p) = query_row_map(
            &conn,
            "SELECT id, status FROM entity_creation_proposals WHERE id = ?1",
            &[SqlV::from(proposal_id.to_string())],
        )?
        else {
            return Ok(json!({"status": "not_found"}));
        };
        let statut = p.get("status").and_then(Value::as_str).unwrap_or("pending");
        if statut != "pending" {
            return Ok(json!({
                "status": "already_resolved",
                "resolution": statut,
                "proposal_id": proposal_id
            }));
        }
        conn.execute(
            "UPDATE entity_creation_proposals SET status = 'rejected', \
             resolved_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![proposal_id],
        )?;
        Ok(json!({"status": "rejected", "proposal_id": proposal_id}))
    }

    /// Port of `step5_validate_pending`: corroborated pending facts promote.
    pub fn validate_pending(&self, new_facts: &[Value]) -> Result<i64, CoreError> {
        let conn = self.storage.lock()?;
        let pending: Vec<(String, String)> = {
            let mut stmt = conn.prepare("SELECT id, fact_data FROM pending_facts")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };

        let mut promoted = 0i64;
        for (pending_id, raw) in pending {
            let Ok(pf) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            let corroborator = new_facts.iter().find(|nf| {
                nf.get("predicate") == pf.get("predicate")
                    && nf
                        .get("entity_canonical")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_lowercase()
                        == pf
                            .get("entity_canonical")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_lowercase()
                    && py_str(nf.get("source_inbox_id")) != py_str(pf.get("source_inbox_id"))
            });
            let Some(corroborator) = corroborator else {
                continue;
            };

            let new_conf = compute_confidence(
                persistence_value(&pf),
                corroborator
                    .get("evidence_strength")
                    .and_then(Value::as_str)
                    .unwrap_or("explicit"),
                true,
                2,
            );
            if new_conf <= 0.85 {
                continue;
            }

            let entity_name = pf
                .get("entity_canonical")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let row = find_existing_entity(&conn, entity_name, &[])?;
            // Post-SYN-112 payloads carry uuid strings; a pre-migration
            // number is kept verbatim (same dangling-ref policy as migrate).
            let prov_id: Option<String> = match pf.get("source_inbox_id") {
                Some(Value::Number(n)) => Some(n.to_string()),
                Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                _ => None,
            };
            let entity_id = match row {
                Some(r) => r.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                None => {
                    let id = new_uuid();
                    conn.execute(
                        "INSERT INTO entities (id, canonical_name, provenance_capture_id) \
                         VALUES (?1, ?2, ?3)",
                        params![id, entity_name, prov_id],
                    )?;
                    id
                }
            };
            insert_fact(
                &conn,
                &entity_id,
                pf.get("predicate").and_then(Value::as_str).unwrap_or(""),
                pf.get("value").cloned().unwrap_or(Value::Null),
                new_conf,
                pf.get("source_inbox_id").cloned().unwrap_or(Value::Null),
                persistence_value(&pf),
                prov_id,
                pf.get("category").cloned().unwrap_or(Value::Null),
            )?;
            conn.execute("DELETE FROM pending_facts WHERE id = ?1", params![pending_id])?;
            promoted += 1;
        }
        Ok(promoted)
    }

    // ── step 2 — resolve ────────────────────────────────────────────────

    fn resolve(
        &self,
        conn: &Connection,
        classified: &Value,
        ctx: &RouteContext,
        // SYN-213 — le texte de la capture, pour vérifier qu'une date issue
        // d'un jour NOMMÉ tombe bien sur ce jour.
        capture: &str,
    ) -> Vec<Resolved> {
        let mut out = Vec::new();
        for entity_data in arr(classified.get("entities")) {
            let aliases: Vec<String> = arr(entity_data.get("aliases"))
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect();
            let canonical = entity_data
                .get("canonical_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let existing = find_existing_entity(conn, canonical, &aliases).unwrap_or(None);

            let mut facts = Vec::new();
            for fact in arr(entity_data.get("facts")) {
                let mut fact = fact.clone();
                let predicate = fact.get("predicate").and_then(Value::as_str).unwrap_or("");
                let lowered = predicate.to_lowercase();
                if DATE_PREDICATE_KEYWORDS.iter().any(|kw| lowered.contains(kw)) {
                    if let Some(v) = fact.get("value").and_then(Value::as_str) {
                        let resolved =
                            resolve_fact_date(v, predicate, &ctx.today, capture);
                        if let Value::Object(m) = &mut fact {
                            m.insert("value".into(), Value::String(resolved));
                        }
                    }
                }
                facts.push(fact);
            }
            out.push(Resolved {
                data: entity_data.clone(),
                existing,
                facts,
            });
        }
        out
    }

    // ── step 4 — route ──────────────────────────────────────────────────

    fn step4_route(
        &self,
        conn: &Connection,
        classified: &Value,
        resolved: &[Resolved],
        source_inbox_id: &str,
        anchors_durable_note: bool,
        ctx: &RouteContext,
    ) -> Result<Vec<String>, CoreError> {
        let mut entity_ids: Vec<String> = Vec::new();

        // Un lien que l'utilisateur a posé lui-même vaut PREUVE : la fiche qui
        // le porte naît sans passer par la file de création. L'exemption tient
        // à l'entité qui reçoit l'URL, jamais à la capture entière — sinon, sur
        // « Ryusuke Hamaguchi https://… », le lien deviendrait un passe-droit
        // pour une personne que rien d'autre ne prouve.
        let porteurs_de_lien: HashSet<String> = arr(classified.get("resources"))
            .iter()
            .filter_map(|r| r.get("entity_canonical").and_then(Value::as_str))
            .map(|n| n.trim().to_lowercase())
            .filter(|n| !n.is_empty())
            .collect();

        let mut relation_names: HashSet<String> = HashSet::new();
        let mut relation_targets_by_from: HashMap<String, HashSet<String>> = HashMap::new();
        for rel in arr(classified.get("relations")) {
            for key in ["from", "to"] {
                if let Some(name) = rel.get(key).and_then(Value::as_str) {
                    if !name.is_empty() {
                        relation_names.insert(name.trim().to_lowercase());
                    }
                }
            }
            let rfrom = rel.get("from").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
            let rto = rel.get("to").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
            if !rfrom.is_empty() && !rto.is_empty() {
                relation_targets_by_from.entry(rfrom).or_default().insert(rto);
            }
        }

        let active_types: HashSet<String> = {
            let mut stmt = conn.prepare("SELECT type FROM active_entity_types")?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter().collect()
        };
        let project_canonicals: HashSet<String> = arr(classified.get("project_entries"))
            .iter()
            .map(|pe| {
                pe.get("project_canonical")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_lowercase()
            })
            .collect();

        for res in resolved {
            let mut entity_data = res.data.clone();
            let canonical = entity_data
                .get("canonical_name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if canonical.is_empty() {
                continue;
            }
            let existing = &res.existing;
            let mention_count = existing
                .as_ref()
                .map(|e| e.get("mention_count").and_then(Value::as_i64).unwrap_or(1) + 1)
                .unwrap_or(1);

            // SYN-58 type guards — new entities only.
            let mut type_proposal: Option<(String, Option<String>)> = None;
            let mut entity_status = "active";
            if existing.is_none() {
                let etype = entity_data
                    .get("type")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("concept")
                    .trim()
                    .to_string();
                if etype == "project" && !project_canonicals.contains(&canonical.to_lowercase()) {
                    if let Value::Object(m) = &mut entity_data {
                        m.insert("type".into(), Value::String("concept".into()));
                    }
                }
                if let Some(tp) = entity_data.get("type_proposal").filter(|v| v.is_object()) {
                    let proposed = tp
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if !proposed.is_empty() && !active_types.contains(&proposed) {
                        let reason = tp.get("reason").and_then(Value::as_str).map(String::from);
                        type_proposal = Some((proposed, reason));
                        entity_status = "pending";
                    }
                }
            }

            // Fact scoring + anti-redite dedup.
            let empty: HashSet<String> = HashSet::new();
            let rel_targets = relation_targets_by_from
                .get(&canonical.to_lowercase())
                .unwrap_or(&empty);
            let mut scored: Vec<(Value, f64)> = Vec::new();
            for fact in &res.facts {
                let value_lower = fact
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_lowercase();
                if rel_targets.contains(&value_lower) {
                    continue; // covered by a relation edge (anti-redite)
                }
                let confidence = compute_confidence(
                    persistence_value(fact),
                    fact.get("evidence_strength")
                        .and_then(Value::as_str)
                        .unwrap_or("explicit"),
                    existing.is_some(),
                    mention_count,
                );
                scored.push((fact.clone(), confidence));
            }

            // Entity creation, decoupled from fact confidence.
            let has_facts = !res.facts.is_empty();
            let max_persistence = if has_facts {
                entity_persistence(&res.facts)
            } else {
                0.0
            };
            // Trois clauses reposent sur une PREUVE — l'entité est déjà
            // connue, elle tient dans un lien, ou l'un de ses faits est assez
            // durable pour la porter. La quatrième n'en a aucune : elle est
            // seulement nommée dans une capture qui laisse une note durable.
            // C'est celle-là qui fabriquait une fiche sur « J'ai la fête de
            // Pierre le 20 », et c'est celle-là qui passe en proposition.
            //
            // Elle n'est pas retirée, et ce n'est pas un détail : sans elle,
            // une entité qui ancre une note durable retombe sous le garde-fou
            // anti-bruit et disparaît sans laisser de trace. Proposer garde la
            // trace ET rend la main.
            // La persistance mesure la NATURE de ce qui est affirmé, pas ce
            // qu'on sait de l'entité. Un salon est durable en soi, donc
            // « Vivatech c'est le 24 » fabriquait une fiche sur une seule
            // mention et un seul fait, qui n'était que la date redite. Les
            // trois autres clauses, elles, prouvent quelque chose : l'entité
            // est déjà connue, elle tient dans un lien, ou elle porte une URL
            // que l'utilisateur a posée lui-même.
            //
            // Donc le PALIER monte quand tout est neuf à la fois : entité
            // inconnue, vue une seule fois, un seul fait. Deux faits, une
            // deuxième mention, ou un fait vraiment durable, et la fiche naît
            // comme avant.
            let connue = existing.is_some();
            let dans_un_lien = relation_names.contains(&canonical.to_lowercase());
            let porte_un_lien = porteurs_de_lien.contains(&canonical.to_lowercase());
            // Un fait qui n'est QUE la date de l'occurrence ne dit rien de
            // l'entité au-delà du fait qu'elle a lieu ce jour-là. C'est la
            // « mention unique sans le moindre détail » de l'arbitrage, et
            // c'est un discriminant stable, contrairement à la persistance :
            // mesuré le 2026-08-28, le modèle sort 3 ou 4 sur la MÊME capture
            // d'une passe à l'autre, donc un palier chiffré tomberait pile sur
            // la bascule.
            let date_redite = res.facts.len() == 1
                && res.facts[0]
                    .get("predicate")
                    .and_then(Value::as_str)
                    .map(|p| {
                        let p = p.trim().to_lowercase();
                        p == "event_date" || p == "occurs_on"
                    })
                    .unwrap_or(false);
            let seule_au_monde = !connue
                && !dans_un_lien
                && !porte_un_lien
                && res.facts.len() <= 1
                && mention_count <= 1;
            let palier = if seule_au_monde {
                LONE_ENTITY_PERSISTENCE
            } else {
                MIN_ENTITY_PERSISTENCE
            };
            let preuve = connue
                || dans_un_lien
                || porte_un_lien
                || (max_persistence >= palier && !(seule_au_monde && date_redite));
            let should_create =
                preuve || (anchors_durable_note && creation_directe_sans_preuve());

            let propose = !should_create && anchors_durable_note;
            if propose {
                record_creation_proposal(
                    conn,
                    &canonical,
                    &entity_data,
                    &res.facts,
                    source_inbox_id,
                    type_proposal.as_ref().map(|(t, _)| t.as_str()),
                )?;
            }

            let mut entity_id: Option<String> = None;
            if should_create {
                let id = upsert_entity(
                    conn,
                    &entity_data,
                    existing.as_ref(),
                    &res.facts,
                    source_inbox_id,
                    entity_status,
                    &ctx.today,
                )?;
                if !entity_ids.contains(&id) {
                    entity_ids.push(id.clone());
                }
                if existing.is_none() {
                    if let Some((proposed, reason)) = &type_proposal {
                        conn.execute(
                            "INSERT INTO entity_type_proposals \
                             (id, proposed_type, reason, evidence_capture_id, candidate_entity_id) \
                             VALUES (?1,?2,?3,?4,?5)",
                            params![new_uuid(), proposed, reason, source_inbox_id, id],
                        )?;
                    }
                    // Python `entity_data.get("type", "concept")`: missing →
                    // "concept", an explicit null stays None (SQL `type = NULL`
                    // matches nothing; the embedding fallback then searches all
                    // types because type_filter=None).
                    let merge_type = match entity_data.get("type") {
                        None => Some("concept"),
                        Some(Value::Null) => None,
                        Some(v) => v.as_str(),
                    };
                    self.propose_merge_if_similar(
                        conn,
                        &id,
                        &canonical,
                        merge_type,
                        source_inbox_id,
                    )?;
                }
                // SYN-188 — un renommage déclaré en capture PROPOSE, il
                // n'applique pas. Réservé aux entités DÉJÀ connues : sur une
                // entité que cette capture vient de créer, il n'y a rien à
                // renommer, elle porte déjà le nom qu'on lui a donné.
                if existing.is_some() {
                    if let Some(nouveau) = entity_data
                        .get("renamed_to")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case(&canonical))
                    {
                        record_rename_proposal(
                            conn, &id, &canonical, nouveau, source_inbox_id,
                        )?;
                    }
                }
                entity_id = Some(id);
            }

            // Une entité seulement PROPOSÉE n'a pas d'identifiant, donc rien
            // à quoi accrocher un fait. Ses faits voyagent dans la charge de
            // la proposition et seront distribués à l'acceptation, par la
            // fonction ci-dessous. Les faire passer ici mettrait la même
            // question deux fois devant l'utilisateur : une fois sur l'entité,
            // une fois sur le fait qui la nomme.
            if !propose {
                dispatch_facts(
                    conn,
                    entity_id.as_deref(),
                    &canonical,
                    &scored,
                    source_inbox_id,
                )?;
            }
        }

        // Relations — both endpoints must already exist; confidence-gated.
        let rel_threshold = env_f64(
            "SYNAPSE_REVIEW_CONFIDENCE_THRESHOLD",
            REVIEW_CONFIDENCE_THRESHOLD_DEFAULT,
        );
        for rel in arr(classified.get("relations")) {
            let from_name = rel.get("from").and_then(Value::as_str).unwrap_or("");
            let predicate = rel.get("predicate").and_then(Value::as_str).unwrap_or("");
            let to_name = rel.get("to").and_then(Value::as_str).unwrap_or("");
            if from_name.is_empty() || predicate.is_empty() || to_name.is_empty() {
                continue;
            }
            let rel_conf = py_float(rel.get("confidence")).unwrap_or(1.0);
            let review_status = if rel_conf < rel_threshold { "pending" } else { "confirmed" };
            let lookup = |name: &str| -> Result<Option<String>, CoreError> {
                let mut stmt = conn.prepare(
                    "SELECT id FROM entities WHERE LOWER(canonical_name) = LOWER(?1)",
                )?;
                let mut rows = stmt.query(params![name])?;
                Ok(match rows.next()? {
                    Some(row) => Some(row.get(0)?),
                    None => None,
                })
            };
            if let (Some(from_id), Some(to_id)) = (lookup(from_name)?, lookup(to_name)?) {
                conn.execute(
                    "INSERT INTO relations \
                     (id, entity_from, predicate, entity_to, confidence, review_status, \
                      provenance_capture_id) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![new_uuid(), from_id, predicate, to_id, rel_conf, review_status,
                            source_inbox_id],
                )?;
            }
        }

        // Les ressources en DERNIER : une URL appartient à quelque chose, et ce
        // quelque chose doit déjà exister pour la recevoir.
        record_resources(conn, classified, source_inbox_id)?;

        // SYN-190 — après TOUTES les écritures, jamais pendant.
        self.propose_predicate_merges(conn, classified, source_inbox_id)?;

        Ok(entity_ids)
    }

    // ── SYN-189 — fact negation ─────────────────────────────────────────

    /// Apply what the capture says has STOPPED being true.
    ///
    /// A negation is the SYN-37 supersede without a successor: the same
    /// machinery, minus the new value. It never deletes. `obsoleted_at` is set
    /// and `obsoleted_by` stays NULL — nothing replaced the fact, it simply
    /// ceased — and `POST /fact/{id}/restore` puts it back. That reversibility
    /// is the whole reason applying on the spot is defensible at all.
    ///
    /// On the spot ONLY when the target is certain. Everything else becomes a
    /// proposal, because the mistake available to this pass is to hide a true
    /// fact, and a hidden fact is not something anyone notices is missing.
    fn apply_negations(
        &self,
        conn: &Connection,
        classified: &Value,
        capture_id: &str,
    ) -> Result<NegationOutcome, CoreError> {
        let mut out = NegationOutcome::default();
        for item in arr(classified.get("obsoleted_facts")) {
            let canonical = item
                .get("entity_canonical")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let predicate = item
                .get("predicate")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if canonical.is_empty() || predicate.is_empty() {
                continue;
            }
            // An absent value and an empty one say the same thing here: the
            // capture named the claim, not which of its values it meant.
            let value = item
                .get("value")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(String::from);
            match self.negate_one(conn, &canonical, &predicate, value.as_deref(), capture_id)? {
                NegationVerdict::Applied(n) => out.applied += n,
                NegationVerdict::Proposed => out.proposed += 1,
                NegationVerdict::Nothing => out.unmatched += 1,
            }
        }
        Ok(out)
    }

    fn negate_one(
        &self,
        conn: &Connection,
        canonical: &str,
        predicate: &str,
        value: Option<&str>,
        capture_id: &str,
    ) -> Result<NegationVerdict, CoreError> {
        let entity_id = match find_existing_entity(conn, canonical, &[])?
            .and_then(|row| row.get("id").and_then(Value::as_str).map(String::from))
        {
            Some(id) => id,
            // Nothing was ever recorded about this entity, so nothing about it
            // can have stopped being true. A negation NEVER creates a node, and
            // never writes a "negative fact" (SYN-189): silence is the answer.
            None => return Ok(NegationVerdict::Nothing),
        };

        // Every live fact on the entity, minus what THIS capture just wrote: a
        // capture does not get to negate its own writes, and the same text can
        // legitimately state a new value and retire an old one.
        let live: Vec<(String, String, String)> = {
            let mut stmt = conn.prepare(
                "SELECT id, LOWER(TRIM(predicate)), LOWER(TRIM(value)) FROM facts \
                 WHERE entity_id = ?1 AND obsoleted_at IS NULL AND archived_at IS NULL \
                 AND COALESCE(provenance_capture_id, '') <> ?2",
            )?;
            let rows = stmt
                .query_map(params![entity_id, capture_id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        if live.is_empty() {
            return Ok(NegationVerdict::Nothing);
        }

        // The CLAIM, not the word. Negating `works_at` has to reach `employer`
        // too — exactly the reach a new value would have had through supersede.
        // Outside a family the predicate is taken literally, which is the
        // residual SYN-190 leaves behind and the reason it blocked this ticket.
        let family: Vec<String> = match single_valued_family(predicate) {
            Some(f) => f.iter().map(|p| (*p).to_string()).collect(),
            None => vec![predicate.trim().to_lowercase()],
        };
        let on_predicate: Vec<(String, String, String)> = live
            .iter()
            .filter(|(_, p, _)| family.iter().any(|f| f == p))
            .cloned()
            .collect();

        if on_predicate.is_empty() {
            // SYN-190's signature, reused as a LAST resort: `worked_at` against
            // `works_at` outside any family. Close enough to be worth showing,
            // never close enough to act on — that is the same measurement that
            // stops the predicate pass from merging on its own authority.
            let sig = Self::predicate_signature(predicate);
            let near: Vec<String> = live
                .iter()
                .filter(|(_, p, _)| Self::predicate_signature(p) == sig)
                .map(|(id, _, _)| id.clone())
                .collect();
            let reason = if near.is_empty() { "introuvable" } else { "approximatif" };
            record_negation_proposal(
                conn, &entity_id, predicate, value, reason, &near, capture_id,
            )?;
            return Ok(NegationVerdict::Proposed);
        }

        let targets: Vec<String> = match value {
            Some(v) => {
                let needle = v.trim().to_lowercase();
                on_predicate
                    .iter()
                    .filter(|(_, _, val)| *val == needle)
                    .map(|(id, _, _)| id.clone())
                    .collect()
            }
            // No value named means the whole claim stopped holding ("il n'a
            // plus de téléphone"). Retiring every live value of that claim IS
            // the certain reading, not an ambiguous one.
            None => on_predicate.iter().map(|(id, _, _)| id.clone()).collect(),
        };

        if targets.is_empty() {
            // The claim is on file, the value is not: the capture and the
            // memory disagree about WHAT was true. Retiring the stored value
            // would be settling a contradiction we have not looked at.
            let candidates: Vec<String> =
                on_predicate.iter().map(|(id, _, _)| id.clone()).collect();
            record_negation_proposal(
                conn,
                &entity_id,
                predicate,
                value,
                "valeur_differente",
                &candidates,
                capture_id,
            )?;
            return Ok(NegationVerdict::Proposed);
        }

        for fact_id in &targets {
            conn.execute(
                "UPDATE facts SET obsoleted_at = CURRENT_TIMESTAMP WHERE id = ?1",
                params![fact_id],
            )?;
        }
        conn.execute(
            "UPDATE entities SET summary_stale = 1 WHERE id = ?1",
            params![entity_id],
        )?;
        Ok(NegationVerdict::Applied(targets.len() as i64))
    }

    // ── SYN-190 — predicate reconciliation ──────────────────────────────

    /// The comparable form of a predicate: empty affixes stripped, verbs cut back
    /// to a stem, words sorted. `is_cousin_of` and `cousin_of` collapse onto the
    /// same signature, `worked_at` and `works_at` too.
    ///
    /// Deliberately timid. An aggressive stemmer would fuse `born_on` with
    /// `borrows`, and a wrong merge costs far more than a surviving duplicate.
    /// It is also why the embedding pass below exists: the signature MISSES
    /// `works_as` → `works_at`, measured, and that is the flagship case.
    fn predicate_signature(predicate: &str) -> String {
        let mut p = predicate.trim().to_lowercase();
        for pre in ["is_", "has_", "was_", "were_"] {
            if let Some(rest) = p.strip_prefix(pre) {
                p = rest.to_string();
                break;
            }
        }
        for suf in ["_of", "_to", "_for"] {
            if let Some(rest) = p.strip_suffix(suf) {
                p = rest.to_string();
                break;
            }
        }
        let mut mots: Vec<String> = Vec::new();
        for m in p.split('_') {
            if m.is_empty() || matches!(m, "the" | "a" | "an") {
                continue;
            }
            let mut m = m.to_string();
            for term in ["ing", "ed", "es", "s"] {
                if m.len() > 4 && m.ends_with(term) {
                    m.truncate(m.len() - term.len());
                    break;
                }
            }
            mots.push(m);
        }
        mots.sort();
        mots.join(" ")
    }

    /// Two predicates differing by exactly ONE token IN THE SAME POSITION are not
    /// synonyms — they are one claim carrying two different values
    /// (`supports_manual_tagging` / `supports_automatic_tagging`,
    /// `is_primary_channel_for` / `is_secondary_channel_for`). Merging them would
    /// DESTROY information, yet this is precisely the family the embedding scores
    /// highest: 0.92 on the pair above, measured 2026-08-24. So it is filtered out
    /// before it can ever reach the queue.
    ///
    /// The right repair for them is to widen the predicate and move the odd word
    /// into `value` — a rewrite, not a merge, and one that only holds when the
    /// value slot is free. `scripts/predicats.py` proposes those separately.
    fn is_sibling_pair(a: &str, b: &str) -> bool {
        let (ta, tb): (Vec<&str>, Vec<&str>) = (a.split('_').collect(), b.split('_').collect());
        if ta.len() != tb.len() || ta.len() < 3 {
            return false;
        }
        ta.iter().zip(tb.iter()).filter(|(x, y)| x != y).count() == 1
    }

    /// A predicate this capture used for the FIRST time, compared to those already
    /// in use. Runs after every fact and relation is written, never inside
    /// `insert_fact`: that one executes inside the caller's open transaction while
    /// the embedder writes on the core's own connection, which is the documented
    /// SQLITE_BUSY trap.
    ///
    /// It only ever PROPOSES. Accepting a merge toward a single-valued family head
    /// (`works_as` → `works_at`) triggers the SYN-37 supersede and obsoletes the
    /// previous fact: doing that unattended would delete knowledge in silence.
    fn propose_predicate_merges(
        &self,
        conn: &Connection,
        classified: &Value,
        capture_id: &str,
    ) -> Result<(), CoreError> {
        let mut vus: HashSet<(String, String)> = HashSet::new();
        for entity in arr(classified.get("entities")) {
            for fact in arr(entity.get("facts")) {
                if let Some(p) = fact.get("predicate").and_then(Value::as_str) {
                    vus.insert(("fact".to_string(), p.trim().to_lowercase()));
                }
            }
        }
        for rel in arr(classified.get("relations")) {
            if let Some(p) = rel.get("predicate").and_then(Value::as_str) {
                vus.insert(("relation".to_string(), p.trim().to_lowercase()));
            }
        }

        let seuil = env_f64(
            "SYNAPSE_PREDICATE_MERGE_THRESHOLD",
            PREDICATE_MERGE_THRESHOLD_DEFAULT,
        );
        for (kind, predicate) in vus {
            if predicate.is_empty() {
                continue;
            }
            let table = if kind == "fact" { "facts" } else { "relations" };
            // Inédit = aucune autre capture ne l'a jamais employé. Sans ce garde,
            // chaque capture rejouerait la comparaison sur tout le vocabulaire.
            let ailleurs: i64 = conn.query_row(
                &format!(
                    "SELECT COUNT(*) FROM {table} WHERE predicate = ?1 \
                     AND COALESCE(provenance_capture_id, '') <> ?2"
                ),
                params![predicate, capture_id],
                |r| r.get(0),
            )?;
            if ailleurs > 0 {
                continue;
            }
            self.propose_one_predicate(conn, &kind, table, &predicate, capture_id, seuil)?;
        }
        Ok(())
    }

    fn propose_one_predicate(
        &self,
        conn: &Connection,
        kind: &str,
        table: &str,
        predicate: &str,
        capture_id: &str,
        seuil: f64,
    ) -> Result<(), CoreError> {
        let sql = format!(
            "SELECT DISTINCT predicate FROM {table} \
             WHERE predicate IS NOT NULL AND predicate <> ?1"
        );
        let mut stmt = conn.prepare(&sql)?;
        let voisins: Vec<String> = stmt
            .query_map(params![predicate], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        if voisins.is_empty() {
            return Ok(());
        }

        // 1. Signature lexicale — précise, gratuite, mais faible en rappel.
        let sig = Self::predicate_signature(predicate);
        for v in &voisins {
            if Self::predicate_signature(v) == sig {
                return record_predicate_proposal(
                    conn, kind, predicate, v, 0.95, "signature", capture_id,
                );
            }
        }

        // 2. Rattrapage sémantique, RESTREINT AUX FAMILLES MONO-VALUÉES.
        //
        // C'est ce qui attrape `works_as` → `works_at`, que la signature rate. Et
        // c'est volontairement tout ce qu'il attrape : hors des familles, la
        // ressemblance des noms ne distingue pas un synonyme d'un inverse (voir
        // la mesure sur PREDICATE_MERGE_THRESHOLD_DEFAULT). Ici l'enjeu est net —
        // un synonyme d'une tête de famille CASSE le supersede, donc chaque
        // proposition répare un bug réel plutôt que d'exprimer un goût.
        if single_valued_family(predicate).is_some() {
            return Ok(()); // déjà dans une famille : rien à réconcilier
        }
        let Ok(cible) = self.embed_text(&predicate.replace('_', " ")) else {
            return Ok(()); // embed indisponible → on saute, comme la fusion d'entités
        };
        let mut meilleur: Option<(f64, &'static str)> = None;
        for v in &voisins {
            let Some(famille) = single_valued_family(v) else {
                continue;
            };
            if Self::is_sibling_pair(predicate, v) {
                continue;
            }
            let Ok(vv) = self.embed_text(&v.replace('_', " ")) else {
                continue;
            };
            let score: f64 = cible
                .iter()
                .zip(vv.iter())
                .map(|(x, y)| (*x as f64) * (*y as f64))
                .sum();
            // La cible est la TÊTE de la famille, jamais le membre rencontré :
            // c'est elle que `insert_fact` sait périmer.
            if score >= seuil && meilleur.map_or(true, |(s, _)| score > s) {
                meilleur = Some((score, famille[0]));
            }
        }
        if let Some((score, tete)) = meilleur {
            let reason = format!("famille_{score:.2}");
            record_predicate_proposal(conn, kind, predicate, tete, score, &reason, capture_id)?;
        }
        Ok(())
    }

    // ── merge + attach proposals ────────────────────────────────────────

    fn propose_merge_if_similar(
        &self,
        conn: &Connection,
        new_id: &str,
        new_name: &str,
        new_type: Option<&str>,
        capture_id: &str,
    ) -> Result<(), CoreError> {
        if new_name.is_empty() {
            return Ok(());
        }
        let needle = new_name.to_lowercase().trim().to_string();
        let needle_tokens: HashSet<&str> = needle.split_whitespace().collect();
        let candidates: Vec<(String, String)> = {
            let mut stmt = conn.prepare(
                "SELECT id, canonical_name FROM entities \
                 WHERE id != ?1 AND type = ?2 AND merged_into_id IS NULL",
            )?;
            let type_param: SqlV = match new_type {
                Some(t) => SqlV::Text(t.to_string()),
                None => SqlV::Null,
            };
            let rows = stmt
                .query_map(params![new_id, type_param], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default()))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (cid, cname) in &candidates {
            let ex_lower = cname.trim().to_lowercase();
            if ex_lower == needle {
                continue;
            }
            if !(ex_lower.contains(&needle) || needle.contains(&ex_lower)) {
                continue;
            }
            let ex_tokens: HashSet<&str> = ex_lower.split_whitespace().collect();
            if needle_tokens.is_disjoint(&ex_tokens) {
                continue;
            }
            if record_merge_proposal(conn, new_id, cid, 0.9, "name_substring", capture_id)? {
                return Ok(());
            }
        }

        // SYN-61 embedding fallback.
        let threshold = env_f64(
            "SYNAPSE_MERGE_EMBEDDING_THRESHOLD",
            MERGE_EMBEDDING_THRESHOLD_DEFAULT,
        );
        let entity = query_row_map(conn, "SELECT * FROM entities WHERE id = ?1", &[SqlV::from(new_id.to_string())])?;
        let Some(entity) = entity else { return Ok(()) };
        let Some(vec) = self.embed(&entity_embedding_text(&entity)) else {
            return Ok(()); // embed failure → skipped, like Python
        };
        let matches = search_entities_on(conn, &vec, 5, threshold, new_type,
                                         &[new_id.to_string()])?;
        for m in &matches {
            let reason = format!("embedding_{:.2}", m.score);
            if record_merge_proposal(conn, new_id, &m.id, m.score, &reason, capture_id)? {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Une capture annule une action : on retrouve la tâche visée, ou on
    /// demande.
    ///
    /// Le symétrique de `negate_one`, côté note, avec une différence qui n'est
    /// pas un détail : un fait se nie par un PRÉDICAT, une tâche par du texte
    /// libre. « appeler le dentiste » et « prendre RDV chez le dentiste » sont
    /// la même intention écrite deux fois, et aucune égalité de chaîne ne le
    /// voit. C'est donc une recherche par proximité, avec deux garde-fous : un
    /// score plancher, et une AVANCE sur la deuxième candidate. Deux tâches qui
    /// se ressemblent, c'est exactement le cas où il ne faut pas choisir seul.
    ///
    /// Sans embarqueur, on ne devine pas : la note de la décision est écrite de
    /// toute façon, donc rien n'est perdu, seul l'archivage n'a pas lieu.
    fn cancel_matching_task(
        &self,
        conn: &Connection,
        action: &str,
        capture_id: &str,
        exclude_note_id: Option<&str>,
    ) -> Result<(i64, i64), CoreError> {
        let action = action.trim();
        if action.is_empty() {
            return Ok((0, 0));
        }
        let Some(vec) = self.embed(action) else {
            return Ok((0, 0));
        };
        let hits = search_live_tasks_on(conn, &vec, 4, exclude_note_id)?;
        if hits.is_empty() {
            // Rien à annuler : aucune tâche vivante ne ressemble à ça. Comme
            // pour un fait jamais enregistré, le silence est la réponse — on
            // ne fabrique pas une question sur une tâche qui n'existe pas.
            return Ok((0, 0));
        }
        let threshold = env_f64("SYNAPSE_TASK_CANCEL_THRESHOLD", TASK_CANCEL_THRESHOLD_DEFAULT);
        let margin = env_f64("SYNAPSE_TASK_CANCEL_MARGIN", TASK_CANCEL_MARGIN_DEFAULT);

        match decide_cancellation(&hits, threshold, margin) {
            CancelDecision::Ask { reason, candidates } => {
                conn.execute(
                    "INSERT INTO note_cancellation_proposals \
                     (id, cancelled_action, reason, candidate_note_ids, evidence_capture_id) \
                     VALUES (?1,?2,?3,?4,?5)",
                    params![
                        new_uuid(),
                        action,
                        reason,
                        serde_json::to_string(&candidates).unwrap_or_else(|_| "[]".into()),
                        capture_id
                    ],
                )?;
                Ok((0, 1))
            }
            CancelDecision::Archive(note_id) => {
                // Archivée, pas supprimée : `POST /atomic-note/{id}/unarchive`
                // existe déjà, donc l'erreur se répare. C'est la condition qui
                // rend l'action acceptable ici.
                conn.execute(
                    "UPDATE atomic_notes SET archived_at = CURRENT_TIMESTAMP, \
                     updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                    params![note_id],
                )?;
                // La même table sert de JOURNAL, avec `status = 'applied'`.
                // Sans elle, une tâche disparaîtrait du backlog sans que rien
                // ne dise pourquoi ni depuis quelle capture, et « annulée »
                // ne se distinguerait plus de « perdue ».
                conn.execute(
                    "INSERT INTO note_cancellation_proposals \
                     (id, cancelled_action, reason, candidate_note_ids, \
                      evidence_capture_id, status, resolved_at, resolved_note_id) \
                     VALUES (?1,?2,'certain',?3,?4,'applied',CURRENT_TIMESTAMP,?5)",
                    params![
                        new_uuid(),
                        action,
                        serde_json::to_string(&[&note_id]).unwrap_or_else(|_| "[]".into()),
                        capture_id,
                        note_id
                    ],
                )?;
                Ok((1, 0))
            }
        }
    }

    fn propose_project_attach_if_similar(
        &self,
        conn: &Connection,
        capture_id: &str,
        content: &str,
        note_id: Option<&str>,
    ) -> Result<bool, CoreError> {
        if content.trim().is_empty() {
            return Ok(false);
        }
        let already: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_entries WHERE capture_id = ?1",
            params![capture_id],
            |r| r.get(0),
        )?;
        if already > 0 {
            return Ok(false);
        }
        let threshold = env_f64("SYNAPSE_PROJECT_ATTACH_THRESHOLD", PROJECT_ATTACH_THRESHOLD_DEFAULT);
        let margin = env_f64("SYNAPSE_PROJECT_ATTACH_MARGIN", PROJECT_ATTACH_MARGIN_DEFAULT);
        let Some(vec) = self.embed(content) else {
            return Ok(false);
        };
        let matches = search_entities_on(conn, &vec, 2, 0.0, Some("project"), &[])?;
        if matches.is_empty() || matches[0].score < threshold {
            return Ok(false);
        }
        if matches.len() > 1 && (matches[0].score - matches[1].score) < margin {
            return Ok(false);
        }
        let m = &matches[0];
        let dup: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_attach_proposals \
             WHERE capture_id = ?1 AND project_id = ?2 AND status = 'pending'",
            params![capture_id, m.id],
            |r| r.get(0),
        )?;
        if dup > 0 {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO project_attach_proposals \
             (id, capture_id, note_id, project_id, content, similarity_score) \
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![new_uuid(), capture_id, note_id, m.id, content.trim(), m.score],
        )?;
        Ok(true)
    }
}

/// Ce qu'on fait d'une annulation, une fois les candidates trouvées.
#[derive(Debug, PartialEq)]
enum CancelDecision {
    /// Une seule candidate, assez proche et assez détachée des autres.
    Archive(String),
    /// Trop loin, ou deux candidates au coude à coude : on demande.
    Ask { reason: &'static str, candidates: Vec<String> },
}

/// La règle, séparée de la base pour se mesurer sans embarqueur ni vecteurs.
///
/// Deux façons de ne pas être sûr, et elles n'ont pas le même motif. « Trop
/// loin » veut dire qu'aucune tâche ne ressemble vraiment à ce qui est annulé.
/// « Au coude à coude » veut dire que deux tâches y ressemblent autant, et
/// c'est le cas où choisir seul est le plus coûteux.
///
/// `hits` arrive trié par score décroissant et n'est jamais vide.
fn decide_cancellation(hits: &[TaskHit], threshold: f64, margin: f64) -> CancelDecision {
    let trop_loin = hits[0].score < threshold;
    let trop_serre = hits.len() > 1 && (hits[0].score - hits[1].score) < margin;
    if !trop_loin && !trop_serre {
        return CancelDecision::Archive(hits[0].note_id.clone());
    }
    // Les candidates montrées sont celles qui valent le coup d'œil. Le
    // plancher est plus bas que le seuil d'action : la question n'engage
    // rien, contrairement à l'archivage.
    let candidates: Vec<String> = hits
        .iter()
        .filter(|h| h.score >= threshold * 0.75)
        .map(|h| h.note_id.clone())
        .collect();
    let candidates = if candidates.is_empty() {
        vec![hits[0].note_id.clone()]
    } else {
        candidates
    };
    CancelDecision::Ask {
        reason: if trop_loin { "approximatif" } else { "ambigu" },
        candidates,
    }
}

struct Resolved {
    data: Value,
    existing: Option<Map<String, Value>>,
    facts: Vec<Value>,
}

// ── shared helpers (ports of the module-level Python functions) ─────────

pub(crate) fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn finish_txn(conn: &Connection, r: Result<(), CoreError>) -> Result<(), CoreError> {
    match r {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Python truthiness over JSON values (None/False/0/""/[]/{} are false).
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

fn arr(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// Python `float(x)` over a JSON value: number, numeric string or bool;
/// anything else (incl. missing/null) is the caller's fallback.
fn py_float(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Python `str(x)` for the source-id comparison in step5 (None → "None").
fn py_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => if *b { "True".into() } else { "False".into() },
        Some(other) => other.to_string(),
    }
}

fn persistence_value(fact: &Value) -> i64 {
    fact.get("persistence_value").and_then(Value::as_i64).unwrap_or(3)
}

/// `_entity_persistence`: strongest persistence among the facts, 3 if none.
fn entity_persistence(facts: &[Value]) -> f64 {
    if facts.is_empty() {
        return 3.0;
    }
    facts
        .iter()
        .map(|f| persistence_value(f) as f64)
        .fold(f64::NEG_INFINITY, f64::max)
}

pub(crate) fn compute_confidence(
    persistence: i64,
    evidence_strength: &str,
    existing: bool,
    mention_count: i64,
) -> f64 {
    let base = match evidence_strength {
        "hedged" => 0.65,
        "implicit" => 0.40,
        _ => 0.92, // explicit + unknown values fall back to explicit
    };
    let mut bonus = 0.0_f64;
    if existing {
        bonus += 0.05;
    }
    bonus += (mention_count as f64 * 0.02).min(0.05);
    bonus += match persistence {
        5 => 0.2,
        4 => 0.15,
        3 => 0.05,
        2 => 0.0,
        1 => -0.1,
        _ => 0.0,
    };
    let mut score = base + bonus;
    if evidence_strength == "hedged" {
        score = score.min(0.84);
    }
    score.clamp(0.0, 1.0)
}

/// Generic row → JSON map (blobs become Null — never consumed by routing).
pub(crate) fn query_row_map(
    conn: &Connection,
    sql: &str,
    params: &[SqlV],
) -> Result<Option<Map<String, Value>>, CoreError> {
    let mut stmt = conn.prepare(sql)?;
    let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let mut rows = stmt.query(params_from_iter(params.iter().cloned()))?;
    match rows.next()? {
        Some(row) => {
            let mut map = Map::new();
            for (i, col) in columns.iter().enumerate() {
                let v = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(f) => json!(f),
                    rusqlite::types::ValueRef::Text(t) => {
                        Value::String(String::from_utf8_lossy(t).into_owned())
                    }
                    rusqlite::types::ValueRef::Blob(_) => Value::Null,
                };
                map.insert(col.clone(), v);
            }
            Ok(Some(map))
        }
        None => Ok(None),
    }
}

pub(crate) fn query_row_maps(
    conn: &Connection,
    sql: &str,
    params: &[SqlV],
) -> Result<Vec<Map<String, Value>>, CoreError> {
    let mut stmt = conn.prepare(sql)?;
    let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let mut rows = stmt.query(params_from_iter(params.iter().cloned()))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut map = Map::new();
        for (i, col) in columns.iter().enumerate() {
            let v = match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(n) => json!(n),
                rusqlite::types::ValueRef::Real(f) => json!(f),
                rusqlite::types::ValueRef::Text(t) => {
                    Value::String(String::from_utf8_lossy(t).into_owned())
                }
                rusqlite::types::ValueRef::Blob(_) => Value::Null,
            };
            map.insert(col.clone(), v);
        }
        out.push(map);
    }
    Ok(out)
}

/// Port of `_find_existing_entity`: primary SQL-cased lookup, then the
/// Python-cased alias scan (first DB-row match wins).
/// pub(crate): `actions.rs` (SYN-135) resolves validated pending facts
/// alias-aware, exactly like `dream_cycle/validation.py` (SYN-87).
pub(crate) fn find_existing_entity(
    conn: &Connection,
    canonical_name: &str,
    aliases: &[String],
) -> Result<Option<Map<String, Value>>, CoreError> {
    if let Some(row) = query_row_map(
        conn,
        "SELECT * FROM entities WHERE LOWER(canonical_name) = LOWER(?1) \
         AND merged_into_id IS NULL",
        &[SqlV::from(canonical_name.to_string())],
    )? {
        return Ok(Some(row));
    }

    let mut search_names: HashSet<String> = HashSet::new();
    search_names.insert(canonical_name.to_lowercase());
    for a in aliases {
        search_names.insert(a.to_lowercase());
    }
    for entity in query_row_maps(conn, "SELECT * FROM entities WHERE merged_into_id IS NULL", &[])? {
        let entity_aliases: Vec<String> = entity
            .get("aliases")
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default();
        let mut existing_names: HashSet<String> = HashSet::new();
        existing_names.insert(
            entity
                .get("canonical_name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase(),
        );
        for a in &entity_aliases {
            existing_names.insert(a.to_lowercase());
        }
        if !search_names.is_disjoint(&existing_names) {
            return Ok(Some(entity));
        }
    }
    Ok(None)
}

/// Port of `_upsert_entity` (aliases union, attributes merge new-wins,
/// mention bump, MAX persistence; INSERT carries provenance + status).
fn upsert_entity(
    conn: &Connection,
    entity_data: &Value,
    existing: Option<&Map<String, Value>>,
    facts: &[Value],
    capture_id: &str,
    status: &str,
    today: &str,
) -> Result<String, CoreError> {
    let summary = entity_data.get("summary").and_then(Value::as_str);
    let attributes = entity_data
        .get("attributes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let persistence = if facts.is_empty() {
        3.0
    } else {
        entity_persistence(facts)
    };

    if let Some(existing) = existing {
        let entity_id = existing
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let existing_aliases: Vec<String> = existing
            .get("aliases")
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default();
        let mut merged: Vec<String> = existing_aliases;
        for a in arr(entity_data.get("aliases")).iter().filter_map(Value::as_str) {
            if !merged.iter().any(|m| m == a) {
                merged.push(a.to_string());
            }
        }
        let mut merged_attrs: Map<String, Value> = existing
            .get("attributes")
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str::<Map<String, Value>>(s).ok())
            .unwrap_or_default();
        for (k, v) in attributes {
            merged_attrs.insert(k, v); // new keys win
        }
        let new_summary = summary
            .map(String::from)
            .or_else(|| existing.get("summary").and_then(Value::as_str).map(String::from));
        conn.execute(
            "UPDATE entities SET aliases=?1, attributes=?2, summary=?3, \
             mention_count=mention_count+1, last_mentioned=?4, \
             persistence_value=MAX(persistence_value, ?5) WHERE id=?6",
            params![
                serde_json::to_string(&merged).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(&Value::Object(merged_attrs)).unwrap_or_else(|_| "{}".into()),
                new_summary,
                today,
                persistence,
                entity_id,
            ],
        )?;
        Ok(entity_id)
    } else {
        let entity_id = new_uuid();
        let aliases: Vec<String> = arr(entity_data.get("aliases"))
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect();
        conn.execute(
            "INSERT INTO entities \
             (id, type, canonical_name, aliases, attributes, summary, last_mentioned, \
              persistence_value, provenance_capture_id, status) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                entity_id,
                match entity_data.get("type") {
                    None => SqlV::Text("concept".into()),
                    Some(Value::Null) => SqlV::Null,
                    Some(v) => SqlV::Text(v.as_str().unwrap_or("concept").into()),
                },
                entity_data.get("canonical_name").and_then(Value::as_str).unwrap_or(""),
                serde_json::to_string(&aliases).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(&Value::Object(attributes)).unwrap_or_else(|_| "{}".into()),
                summary,
                today,
                persistence,
                capture_id,
                status,
            ],
        )?;
        Ok(entity_id)
    }
}

/// SYN-188 — park a rename declared by a capture.
///
/// Idempotent on (entity, proposed name) while pending: the same capture
/// replayed, or the rename declared twice before anyone confirms, must not
/// stack two identical questions.
/// Enregistrer les liens d'une capture. AUCUN réseau ici.
///
/// C'est le renversement du ticket : une ressource naît de ce que le
/// classifieur a lu, pas de ce qu'une requête a ramené. Aller chercher la page
/// devient un enrichissement qui peut échouer sans rien coûter, au lieu d'être
/// ce qui décidait s'il existait une ressource — c'est ce couplage qui a rempli
/// la mémoire d'un mur de connexion et d'une bannière de cookies.
///
/// Idempotent sur l'URL, et il ne remplit QUE ce qui manque : une capture
/// rejouée, ou un second lien vers la même page, ne doit pas écraser ce qu'un
/// humain a corrigé entre-temps.
fn record_resources(
    conn: &Connection,
    classified: &Value,
    capture_id: &str,
) -> Result<(), CoreError> {
    let texte = |v: Option<&Value>| {
        v.and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(String::from)
    };

    for item in arr(classified.get("resources")) {
        let Some(url) = texte(item.get("url")) else { continue };
        if !url.starts_with("http") {
            continue; // le champ dit « URL » ; ce qui n'en est pas ne l'est pas
        }
        let categorie = texte(item.get("category"));
        let commentaire = texte(item.get("user_comment"));

        let entity_id = match texte(item.get("entity_canonical")) {
            Some(nom) => query_row_map(
                conn,
                "SELECT id FROM entities WHERE LOWER(canonical_name) = LOWER(?1) \
                 AND merged_into_id IS NULL",
                &[SqlV::from(nom)],
            )?
            .and_then(|r| r.get("id").and_then(Value::as_str).map(String::from)),
            None => None,
        };

        let deja = query_row_map(
            conn,
            "SELECT id FROM resources WHERE url = ?1",
            &[SqlV::from(url.clone())],
        )?;
        match deja {
            Some(row) => {
                let rid = row.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                conn.execute(
                    "UPDATE resources SET entity_id = COALESCE(entity_id, ?2), \
                     user_comment = COALESCE(user_comment, ?3) WHERE id = ?1",
                    params![rid, entity_id, commentaire],
                )?;
            }
            None => {
                conn.execute(
                    "INSERT INTO resources (id, type, source, url, entity_id, user_comment) \
                     VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        new_uuid(),
                        categorie.clone().unwrap_or_else(|| "page".into()),
                        capture_id,
                        url,
                        entity_id,
                        commentaire
                    ],
                )?;
            }
        }

        if let Some(eid) = &entity_id {
            poser_le_lien_sur_la_fiche(conn, eid, &url, categorie.as_deref(),
                                       commentaire.as_deref())?;
        }
    }
    Ok(())
}

/// L'URL et la catégorie vont dans `attributes`, pas dans un fait.
///
/// Une URL est une IDENTITÉ, pas une affirmation sur la chose : « ceci est
/// l'adresse de Linear » ne se périme pas, ne se contredit pas et n'a rien à
/// faire dans une file de validation de faits. Et le vocabulaire des prédicats
/// est gouverné (SYN-190) : y ajouter `url` ouvrirait une famille entière pour
/// une donnée qui n'en est pas une.
fn poser_le_lien_sur_la_fiche(
    conn: &Connection,
    entity_id: &str,
    url: &str,
    categorie: Option<&str>,
    commentaire: Option<&str>,
) -> Result<(), CoreError> {
    let Some(row) = query_row_map(
        conn,
        "SELECT attributes, summary FROM entities WHERE id = ?1",
        &[SqlV::from(entity_id.to_string())],
    )?
    else {
        return Ok(());
    };
    let mut attrs: Map<String, Value> = row
        .get("attributes")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    attrs.insert("url".into(), json!(url));
    if let Some(c) = categorie {
        attrs.insert("resource_category".into(), json!(c));
    }

    // Le commentaire de l'auteur devient le résumé de la fiche quand elle n'en
    // a pas. Il dit pourquoi LUI l'a gardée, ce qu'aucun résumé de la page ne
    // saura dire. Il ne remplace jamais un résumé existant.
    let resume_vide = row
        .get("summary")
        .and_then(Value::as_str)
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    match (resume_vide, commentaire) {
        (true, Some(c)) => conn.execute(
            "UPDATE entities SET attributes = ?2, summary = ?3 WHERE id = ?1",
            params![entity_id, py_dumps_ascii(&Value::Object(attrs)), c],
        )?,
        _ => conn.execute(
            "UPDATE entities SET attributes = ?2 WHERE id = ?1",
            params![entity_id, py_dumps_ascii(&Value::Object(attrs))],
        )?,
    };
    Ok(())
}

/// Distribuer les faits d'une entité selon leur confiance.
///
/// Extraite pour que l'acceptation d'une entité proposée emprunte EXACTEMENT
/// ce chemin-là, et pas une copie qui dériverait. Les trois destinations et
/// leurs deux seuils sont la seule chose que cette fonction décide.
///
/// `entity_id` à None : un fait sûr n'a nulle part où aller et se perd. C'est
/// le comportement d'origine, laissé tel quel — le changer serait un autre
/// sujet que celui de cette fonction.
fn dispatch_facts(
    conn: &Connection,
    entity_id: Option<&str>,
    canonical: &str,
    scored: &[(Value, f64)],
    source_inbox_id: &str,
) -> Result<(), CoreError> {
    for (fact, confidence) in scored {
        if *confidence > 0.85 {
            if let Some(eid) = entity_id {
                insert_fact(
                    conn,
                    eid,
                    fact.get("predicate").and_then(Value::as_str).unwrap_or(""),
                    fact.get("value").cloned().unwrap_or(Value::Null),
                    *confidence,
                    Value::String(source_inbox_id.to_string()),
                    persistence_value(fact),
                    Some(source_inbox_id.to_string()),
                    fact.get("category").cloned().unwrap_or(Value::Null),
                )?;
            }
        } else {
            let fact_data = json!({
                "entity_canonical": canonical,
                "predicate": fact.get("predicate"),
                "value": fact.get("value"),
                "persistence_value": fact.get("persistence_value").cloned()
                    .unwrap_or(json!(3)),
                "evidence_strength": fact.get("evidence_strength").cloned()
                    .unwrap_or(json!("explicit")),
                "category": fact.get("category"),
                "confidence": confidence,
                "source_inbox_id": source_inbox_id,
            });
            if *confidence >= 0.5 {
                conn.execute(
                    "INSERT INTO pending_facts (id, fact_data, validation_strategy) \
                     VALUES (?1,?2,?3)",
                    params![new_uuid(), py_dumps_ascii(&fact_data), "passive"],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO review_queue (id, fact_data, suggested_entity) \
                     VALUES (?1,?2,?3)",
                    params![new_uuid(), py_dumps_ascii(&fact_data), canonical],
                )?;
            }
        }
    }
    Ok(())
}

/// Garer une entité que rien ne prouve, au lieu de la créer.
///
/// La charge complète du classifieur est stockée telle quelle : accepter plus
/// tard doit écrire la MÊME entité, avec ses alias, ses attributs et ses faits,
/// sans rejouer la capture — donc sans repayer un appel au modèle pour une
/// réponse qu'on avait déjà.
///
/// Idempotent sur le nom tant que la proposition est en attente : la même
/// capture rejouée, ou le même nom croisé deux fois avant qu'on tranche, ne
/// doit pas empiler deux fois la même question.
fn record_creation_proposal(
    conn: &Connection,
    canonical: &str,
    entity_data: &Value,
    facts: &[Value],
    capture_id: &str,
    proposed_type: Option<&str>,
) -> Result<(), CoreError> {
    if canonical.trim().is_empty() {
        return Ok(());
    }
    let already: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entity_creation_proposals \
         WHERE LOWER(TRIM(canonical_name)) = LOWER(TRIM(?1)) AND status = 'pending'",
        params![canonical],
        |r| r.get(0),
    )?;
    if already > 0 {
        return Ok(());
    }
    // Les faits voyagent DANS la charge : `entity_data` sort du classifieur
    // sans eux, et une entité acceptée sans ses faits serait une fiche vide
    // dont l'utilisateur ne comprendrait pas ce qu'elle fait là.
    let mut charge = entity_data.clone();
    if let Value::Object(m) = &mut charge {
        m.insert("facts".into(), Value::Array(facts.to_vec()));
    }
    let declare = entity_data.get("type").and_then(Value::as_str);
    conn.execute(
        "INSERT INTO entity_creation_proposals \
         (id, canonical_name, proposed_type, entity_data, evidence_capture_id) \
         VALUES (?1,?2,?3,?4,?5)",
        params![
            new_uuid(),
            canonical,
            proposed_type.or(declare),
            py_dumps_ascii(&charge),
            capture_id
        ],
    )?;
    Ok(())
}

fn record_rename_proposal(
    conn: &Connection,
    entity_id: &str,
    current_name: &str,
    proposed_name: &str,
    capture_id: &str,
) -> Result<(), CoreError> {
    let already: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entity_rename_proposals \
         WHERE entity_id = ?1 AND LOWER(TRIM(proposed_name)) = LOWER(TRIM(?2)) \
         AND status = 'pending'",
        params![entity_id, proposed_name],
        |r| r.get(0),
    )?;
    if already > 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO entity_rename_proposals \
         (id, entity_id, current_name, proposed_name, evidence_capture_id) \
         VALUES (?1,?2,?3,?4,?5)",
        params![new_uuid(), entity_id, current_name, proposed_name, capture_id],
    )?;
    Ok(())
}

/// SYN-189 — park a negation whose target is not certain.
///
/// Idempotent on (entity, predicate, value) while pending: the same capture
/// replayed, or the same claim denied twice before anyone arbitrates, must not
/// stack two identical questions in the queue.
#[allow(clippy::too_many_arguments)]
fn record_negation_proposal(
    conn: &Connection,
    entity_id: &str,
    predicate: &str,
    value: Option<&str>,
    reason: &str,
    candidate_fact_ids: &[String],
    capture_id: &str,
) -> Result<(), CoreError> {
    let already: i64 = conn.query_row(
        "SELECT COUNT(*) FROM fact_negation_proposals \
         WHERE entity_id = ?1 AND LOWER(TRIM(predicate)) = LOWER(TRIM(?2)) \
         AND COALESCE(LOWER(TRIM(value)), '') = COALESCE(LOWER(TRIM(?3)), '') \
         AND status = 'pending'",
        params![entity_id, predicate, value],
        |r| r.get(0),
    )?;
    if already > 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO fact_negation_proposals \
         (id, entity_id, predicate, value, reason, candidate_fact_ids, evidence_capture_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            new_uuid(),
            entity_id,
            predicate,
            value,
            reason,
            serde_json::to_string(candidate_fact_ids).unwrap_or_else(|_| "[]".to_string()),
            capture_id
        ],
    )?;
    Ok(())
}

fn record_predicate_proposal(
    conn: &Connection,
    kind: &str,
    candidate: &str,
    existing: &str,
    score: f64,
    reason: &str,
    capture_id: &str,
) -> Result<(), CoreError> {
    // La DIRECTION compte. Si l'un des deux est tête d'une famille mono-valuée,
    // c'est lui la cible : ramener un synonyme vers la tête RÉPARE le supersede,
    // l'inverse le casserait définitivement.
    let (candidate, existing) = if single_valued_family(candidate).is_some()
        && single_valued_family(existing).is_none()
    {
        (existing, candidate)
    } else {
        (candidate, existing)
    };
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM predicate_merge_proposals \
         WHERE kind = ?1 AND ((candidate_predicate=?2 AND existing_predicate=?3) \
                           OR (candidate_predicate=?3 AND existing_predicate=?2))",
        params![kind, candidate, existing],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO predicate_merge_proposals \
         (id, kind, candidate_predicate, existing_predicate, similarity_score, \
          similarity_reason, evidence_capture_id) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![new_uuid(), kind, candidate, existing, score, reason, capture_id],
    )?;
    Ok(())
}

fn record_merge_proposal(
    conn: &Connection,
    new_id: &str,
    existing_id: &str,
    score: f64,
    reason: &str,
    capture_id: &str,
) -> Result<bool, CoreError> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entity_merge_proposals \
         WHERE (candidate_entity_id=?1 AND existing_entity_id=?2) \
            OR (candidate_entity_id=?2 AND existing_entity_id=?1)",
        params![new_id, existing_id],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO entity_merge_proposals \
         (id, candidate_entity_id, existing_entity_id, similarity_score, \
          similarity_reason, evidence_capture_id) VALUES (?1,?2,?3,?4,?5,?6)",
        params![new_uuid(), new_id, existing_id, score, reason, capture_id],
    )?;
    Ok(true)
}

/// Port of `facts_store.insert_fact` (dedup-reinforce + SYN-37 supersede).
/// pub(crate): the SQL gateway re-exposes it on the HOST's connection
/// (`SqlConnection::insert_fact`) so user-action endpoints keep their open
/// transaction (T5 — the Python copy is gone).
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_fact(
    conn: &Connection,
    entity_id: &str,
    predicate: &str,
    value: Value,
    confidence: f64,
    source_inbox_id: Value,
    persistence_value: i64,
    provenance_capture_id: Option<String>,
    category: Value,
) -> Result<String, CoreError> {
    let fact_id = new_uuid();
    let value_sql = json_scalar_to_sql(&value);
    let dup = {
        let mut stmt = conn.prepare(
            "SELECT id, confidence FROM facts \
             WHERE entity_id = ?1 AND LOWER(TRIM(predicate)) = LOWER(TRIM(?2)) \
             AND LOWER(TRIM(value)) = LOWER(TRIM(?3)) \
             AND obsoleted_at IS NULL AND archived_at IS NULL LIMIT 1",
        )?;
        let mut rows = stmt.query(params![entity_id, predicate, value_sql])?;
        match rows.next()? {
            Some(row) => Some((row.get::<_, String>(0)?, row.get::<_, Option<f64>>(1)?)),
            None => None,
        }
    };
    if let Some((dup_id, dup_conf)) = dup {
        conn.execute(
            "UPDATE facts SET confidence = ?1, last_confirmed = CURRENT_TIMESTAMP WHERE id = ?2",
            params![confidence.max(dup_conf.unwrap_or(0.0)), dup_id],
        )?;
        return Ok(dup_id);
    }
    if let Some(family) = single_valued_family(predicate) {
        // Family members are lowercase ASCII identifiers declared above — no
        // caller-supplied text reaches this string.
        let list = family
            .iter()
            .map(|p| format!("'{p}'"))
            .collect::<Vec<_>>()
            .join(",");
        let existing: Vec<(String, Option<f64>)> = {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, confidence FROM facts \
                 WHERE entity_id = ?1 AND LOWER(TRIM(predicate)) IN ({list}) \
                 AND obsoleted_at IS NULL AND archived_at IS NULL",
            ))?;
            let rows = stmt
                .query_map(params![entity_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (ex_id, ex_conf) in existing {
            if confidence >= ex_conf.unwrap_or(0.0) {
                conn.execute(
                    "UPDATE facts SET obsoleted_at = CURRENT_TIMESTAMP, obsoleted_by = ?1 \
                     WHERE id = ?2",
                    params![fact_id, ex_id],
                )?;
            }
        }
    }
    conn.execute(
        "INSERT INTO facts \
         (id, entity_id, predicate, value, confidence, source_inbox_id, \
          persistence_value, provenance_capture_id, category) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            fact_id,
            entity_id,
            predicate,
            value_sql,
            confidence,
            json_scalar_to_sql(&source_inbox_id),
            persistence_value,
            provenance_capture_id,
            json_scalar_to_sql(&category),
        ],
    )?;
    conn.execute(
        "UPDATE entities SET summary_stale = 1 WHERE id = ?1",
        params![entity_id],
    )?;
    Ok(fact_id)
}

/// Bind a JSON scalar like Python bound the native value (str/int/float/
/// bool/None); structures fall back to their compact JSON text.
pub(crate) fn json_scalar_to_sql(v: &Value) -> SqlV {
    match v {
        Value::Null => SqlV::Null,
        Value::Bool(b) => SqlV::Integer(*b as i64),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlV::Integer(i)
            } else {
                SqlV::Real(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => SqlV::Text(s.clone()),
        other => SqlV::Text(other.to_string()),
    }
}

fn mark(conn: &Connection, entry_id: &str, now: &str, status: &str) -> Result<(), CoreError> {
    conn.execute(
        "UPDATE inbox SET processed_at=?1, status=?2, error=NULL WHERE id=?3",
        params![now, status, entry_id],
    )?;
    Ok(())
}

/// Port of `handle_intentions` (expired purge + optional insert).
fn handle_intentions(
    conn: &Connection,
    classified: &Value,
    ctx: &RouteContext,
) -> Result<(), CoreError> {
    conn.execute(
        "DELETE FROM intentions WHERE created_at < ?1 AND resolved = 0",
        params![ctx.intentions_cutoff],
    )?;
    let is_ephemeral = truthy(classified.get("is_ephemeral"));
    if is_ephemeral {
        let source = classified
            .get("ephemeral_content")
            .filter(|v| truthy(Some(v)))
            .cloned()
            .unwrap_or_else(|| classified.get("summary").cloned().unwrap_or(json!("")));
        let content = intention_text(&source);
        if !content.is_empty() {
            conn.execute(
                "INSERT INTO intentions (id, content, ttl_hours) VALUES (?1,?2,?3)",
                params![new_uuid(), content, 48],
            )?;
        }
    }
    Ok(())
}

/// Port of `_intention_text` (dict/list coercion into TEXT).
fn intention_text(value: &Value) -> String {
    let mut v = value.clone();
    if let Value::Object(m) = &v {
        v = m
            .get("content")
            .filter(|x| truthy(Some(x)))
            .or_else(|| m.get("text").filter(|x| truthy(Some(x))))
            .or_else(|| m.get("description").filter(|x| truthy(Some(x))))
            .or_else(|| m.get("items").filter(|x| truthy(Some(x))))
            .cloned()
            .unwrap_or_else(|| Value::String(py_dumps(&v)));
    }
    if let Value::Array(items) = &v {
        let joined: Vec<String> = items
            .iter()
            .filter(|x| truthy(Some(x)))
            .map(py_scalar_str)
            .collect();
        v = Value::String(joined.join(" · "));
    }
    match &v {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        other => py_scalar_str(other).trim().to_string(),
    }
}

/// Python `str()` of a JSON scalar (dicts/lists via py_dumps-ish repr are
/// not needed on the corpus; keep JSON text for them).
fn py_scalar_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "True".into() } else { "False".into() },
        Value::Null => "None".into(),
        other => other.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn persist_atomic_note(
    conn: &Connection,
    content: &str,
    summary: &str,
    entities_mentioned: &[String],
    capture_id: &str,
    kind: &str,
    event_date: Option<&str>,
    event_recurring: bool,
    review_status: &str,
    review_reason: Option<&str>,
    owner: Option<&str>,
    language: Option<&str>,
) -> Result<String, CoreError> {
    let kind = if ["note", "task", "event", "episode"].contains(&kind) { kind } else { "note" };
    let review_status = if ["confirmed", "pending"].contains(&review_status) {
        review_status
    } else {
        "confirmed"
    };
    // A reason without a pending status would be read by the UI as a question
    // to ask about a row nobody is questioning.
    let review_reason = if review_status == "pending" { review_reason } else { None };
    let title: String = if summary.is_empty() { content } else { summary }
        .chars()
        .take(60)
        .collect();
    // SYN-182 — an episode HAS a date by nature; it just never got to keep one.
    // `durable` used to mean "event or task", so "our first meeting with Marie
    // was 18 April" was routed to `episode` (it is past) and then written with
    // event_date = NULL and event_recurring = 0. The recurring meeting-anniversary
    // was destroyed at insert time, not lost further down — no wording of the
    // recurrence rule could have saved it.
    let dated = matches!(kind, "event" | "task" | "episode");
    let note_id = new_uuid();
    conn.execute(
        "INSERT INTO atomic_notes \
         (id, title, content, summary, entities_mentioned, memory_strength, \
          provenance_capture_id, kind, event_date, event_recurring, review_status, \
          review_reason, owner, language) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![
            note_id,
            title,
            content,
            summary,
            serde_json::to_string(entities_mentioned).unwrap_or_else(|_| "[]".into()),
            1.0,
            capture_id,
            kind,
            if dated { event_date } else { None },
            (dated && event_recurring) as i64,
            review_status,
            review_reason,
            owner,
            language,
        ],
    )?;
    Ok(note_id)
}

pub(crate) fn persist_project_entry(
    conn: &Connection,
    canonical: &str,
    content: &str,
    capture_id: &str,
    is_new_project: bool,
) -> Result<ProjectSynthesis, CoreError> {
    let existing: Option<String> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM entities WHERE type='project' AND LOWER(canonical_name) = LOWER(?1)",
        )?;
        let mut rows = stmt.query(params![canonical])?;
        match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        }
    };
    let project_id = match existing {
        Some(id) => {
            conn.execute(
                "UPDATE entities SET mention_count = mention_count + 1, \
                 last_mentioned = DATE('now') WHERE id = ?1",
                params![id],
            )?;
            id
        }
        None => {
            let id = new_uuid();
            conn.execute(
                "INSERT INTO entities \
                 (id, type, canonical_name, mention_count, last_mentioned, persistence_value, \
                  summary, provenance_capture_id) \
                 VALUES (?1, 'project', ?2, 1, DATE('now'), 3, ?3, ?4)",
                params![
                    id,
                    canonical,
                    if is_new_project {
                        Some("Projet créé automatiquement par le Dream Cycle.")
                    } else {
                        None
                    },
                    capture_id
                ],
            )?;
            id
        }
    };
    let entry_id = new_uuid();
    conn.execute(
        "INSERT INTO project_entries (id, project_id, capture_id, content, kind) \
         VALUES (?1, ?2, ?3, ?4, 'note')",
        params![entry_id, project_id, capture_id, content],
    )?;
    let entry_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM project_entries WHERE project_id = ?1",
        params![project_id],
        |r| r.get(0),
    )?;
    Ok(ProjectSynthesis {
        project_id,
        entry_id,
        project_name: canonical.to_string(),
        entry_content: content.to_string(),
        entry_count,
    })
}

/// Port of `entity_embedding_text` — the exact text fastembed/the core
/// embeds for an entity; `py_dumps` keeps Python's JSON byte layout so the
/// vectors stay comparable.
pub(crate) fn entity_embedding_text(entity: &Map<String, Value>) -> String {
    let aliases: Vec<String> = entity
        .get("aliases")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default();
    let attributes: Value = entity
        .get("attributes")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    format!(
        "Nom: {}\nType: {}\nAliases: {}\nAttributs: {}\nRésumé: {}",
        entity.get("canonical_name").and_then(Value::as_str).unwrap_or(""),
        entity.get("type").and_then(Value::as_str).unwrap_or(""),
        aliases.join(", "),
        py_dumps(&attributes),
        entity.get("summary").and_then(Value::as_str).unwrap_or(""),
    )
}

/// `json.dumps(v, ensure_ascii=False)` — Python's default separators
/// (", ", ": ") and insertion order (serde_json preserve_order).
pub(crate) fn py_dumps(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let inner: Vec<String> = m
                .iter()
                .map(|(k, val)| format!("{}: {}", serde_json::to_string(k).unwrap(), py_dumps(val)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        Value::Array(a) => {
            let inner: Vec<String> = a.iter().map(py_dumps).collect();
            format!("[{}]", inner.join(", "))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// `json.dumps(v)` with ensure_ascii=True (pending/review fact_data uses the
/// Python default). Non-ASCII chars are \uXXXX-escaped like CPython.
fn py_dumps_ascii(v: &Value) -> String {
    let raw = py_dumps(v);
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if (c as u32) < 0x80 {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

/// Minimal deterministic stand-in for `dateparser.parse(...).date()`,
/// covering the value shapes the classifier actually produces (it is told
/// to resolve dates itself): ISO dates pass through, a bare year resolves
/// like dateparser does (current month, PREFER_DAY_OF_MONTH=first), and
/// the few English/French relative phrases seen in the wild. Anything else
/// returns unchanged — same as a dateparser miss.
fn resolve_date(value: &str, today: &str) -> String {
    let v = value.trim();
    // Already ISO (date or datetime prefix).
    let bytes = v.as_bytes();
    let is_iso = v.len() >= 10
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    if is_iso {
        return v[0..10].to_string();
    }
    // Bare year → year-<current month>-01 (dateparser fills missing month
    // from the current date and the day from PREFER_DAY_OF_MONTH=first).
    if v.len() == 4 && v.chars().all(|c| c.is_ascii_digit()) {
        return format!("{v}-{}-01", &today[5..7]);
    }
    // Partial ISO month-day ("07-04") → current year prepended (dateparser
    // read these MDY and filled the year from the current date).
    if v.len() == 5
        && bytes[0..2].iter().all(u8::is_ascii_digit)
        && bytes[2] == b'-'
        && bytes[3..5].iter().all(u8::is_ascii_digit)
    {
        return format!("{}-{v}", &today[0..4]);
    }
    let lower = v.to_lowercase();
    if let Some(days) = match lower.as_str() {
        "today" | "aujourd'hui" => Some(0),
        "tomorrow" | "demain" => Some(1),
        "next week" | "la semaine prochaine" => Some(7),
        _ => None,
    } {
        return add_days_iso(today, days);
    }
    // "12 juin", "June 12, 1990", "3 mars 1990". A yearless date takes the
    // current year, like the "07-04" case above: `digest::next_occurrence`
    // reads the month and day only, so the year is an anchor, not a claim.
    if let Some((m, d, y)) = parse_month_name_date(v) {
        let year = y.unwrap_or_else(|| today[0..4].parse().unwrap_or(1970));
        if d <= month_len(year, m as i64) as u32 {
            return format!("{year:04}-{m:02}-{d:02}");
        }
    }
    value.to_string()
}

/// "12 juin" / "June 12, 1990" → (month, day, year?). Deliberately narrow:
/// this normalises the shapes a classifier actually returns, and bails out on
/// anything else rather than guessing. A value it cannot read is left as the
/// model wrote it — visibly unresolved beats silently wrong.
fn parse_month_name_date(value: &str) -> Option<(u32, u32, Option<i64>)> {
    let lower = value.to_lowercase();
    let (mut month, mut day, mut year) = (None, None, None);
    for token in lower.split(|c: char| !c.is_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        if let Some(idx) = MONTH_NAMES.iter().position(|names| names.contains(&token)) {
            if month.replace(idx as u32 + 1).is_some() {
                return None; // two month names — not a date we understand
            }
            continue;
        }
        // "1st", "2nd", "12th" — the ordinal suffix carries nothing.
        let digits = token.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            // A stray article is noise; an unknown word means we misread the value.
            if matches!(token, "le" | "de" | "du" | "the" | "of") {
                continue;
            }
            return None;
        }
        match digits.len() {
            4 if year.replace(digits.parse().ok()?).is_none() => {}
            1 | 2 if day.replace(digits.parse().ok()?).is_none() => {}
            _ => return None,
        }
    }
    let (m, d) = (month?, day?);
    (1..=31).contains(&d).then_some((m, d, year))
}

/// Fact-side date normalisation. On top of [`resolve_date`], a past-only
/// predicate landing in the future is re-anchored backwards: the model
/// resolved "le 23 juillet" to *next* year's occurrence, and storing 2027 as
/// someone's birth year states something plainly false on their fiche.
/// Rolling an anniversary forward is `digest::next_occurrence`'s job and it
/// reads month and day only, so moving the year back costs no notification.
fn resolve_fact_date(value: &str, predicate: &str, today: &str, capture: &str) -> String {
    let iso = resolve_date(value, today);
    // La fenêtre de douze mois ne s'applique PAS à une naissance ni à un
    // anniversaire : l'année d'une date de naissance ne se déduit d'aucun
    // calendrier, et la ramener dans la fenêtre l'écraserait. Le recalage
    // d'année propre à ces prédicats est écrit plus bas, il suffit.
    let iso = if PAST_ONLY_PREDICATE_KEYWORDS
        .iter()
        .any(|kw| predicate.to_lowercase().contains(kw))
    {
        iso
    } else {
        snap_bare_day_month(&iso, capture, today)
    };
    let resolved = snap_to_named_weekday(&iso, capture, today);
    let p = predicate.to_lowercase();
    if !PAST_ONLY_PREDICATE_KEYWORDS.iter().any(|kw| p.contains(kw)) {
        return resolved;
    }
    if resolved.len() != 10 || resolved.as_str() <= today {
        return resolved;
    }
    let (m, d) = (
        resolved[5..7].parse::<i64>().unwrap_or(0),
        resolved[8..10].parse::<i64>().unwrap_or(0),
    );
    let mut year: i64 = today[0..4].parse().unwrap_or(0);
    if format!("{year:04}-{m:02}-{d:02}").as_str() > today {
        year -= 1;
    }
    // 29 February only exists on a leap year; re-anchoring it would produce a
    // date chrono refuses to parse, which would cost the notification outright.
    if d > month_len(year, m) {
        return resolved;
    }
    format!("{year:04}-{m:02}-{d:02}")
}

fn leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_len(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if leap(y) { 29 } else { 28 },
        _ => 0,
    }
}

/// Weekday names the classifier realistically sees in a capture, FR and EN,
/// accented or not, full or abbreviated. Index = ISO weekday − 1 (Monday = 0).
const WEEKDAY_NAMES: [&[&str]; 7] = [
    &["lundi", "monday", "lun", "mon"],
    &["mardi", "tuesday", "mar", "tue", "tues"],
    &["mercredi", "wednesday", "mer", "wed"],
    &["jeudi", "thursday", "jeu", "thu", "thur", "thurs"],
    &["vendredi", "friday", "ven", "fri"],
    &["samedi", "saturday", "sam", "sat"],
    &["dimanche", "sunday", "dim", "sun"],
];

/// ISO weekday of a `YYYY-MM-DD` string, Monday = 0. Sakamoto, no deps, to
/// stay in the style of the arithmetic already in this file.
fn weekday_index(date: &str) -> Option<usize> {
    if date.len() < 10 {
        return None;
    }
    let y: i64 = date[0..4].parse().ok()?;
    let m: i64 = date[5..7].parse().ok()?;
    let d: i64 = date[8..10].parse().ok()?;
    if !(1..=12).contains(&m) || d < 1 || d > month_len(y, m) {
        return None;
    }
    const OFF: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let yy = if m < 3 { y - 1 } else { y };
    // Sakamoto yields Sunday = 0; shift so that Monday = 0.
    let sunday0 = (yy + yy / 4 - yy / 100 + yy / 400 + OFF[(m - 1) as usize] + d) % 7;
    Some(((sunday0 + 6) % 7) as usize)
}

/// The single weekday NAMED in a capture, if there is exactly one.
///
/// Exactly one is the condition. "on s'est vus mardi puis jeudi" names two and
/// nothing here can tell which one the date belongs to, so we keep quiet
/// rather than guess.
fn named_weekday(text: &str) -> Option<usize> {
    let lower = text.to_lowercase();
    let mut found: Option<usize> = None;
    for (i, names) in WEEKDAY_NAMES.iter().enumerate() {
        // Whole word only: "mar" must not match inside "marché", "sun" inside
        // "sunny", "dim" inside "dimanche" (which the same loop matches whole).
        let hit = names.iter().any(|n| {
            lower.match_indices(*n).any(|(at, _)| {
                let before = lower[..at].chars().next_back();
                let after = lower[at + n.len()..].chars().next();
                !before.is_some_and(char::is_alphanumeric)
                    && !after.is_some_and(char::is_alphanumeric)
            })
        });
        if hit {
            if found.is_some() && found != Some(i) {
                return None;
            }
            found = Some(i);
        }
    }
    found
}

/// Does the capture write a YEAR anywhere? If it does, the model had one to
/// read and we keep our hands off.
fn states_a_year(text: &str) -> bool {
    let b = text.as_bytes();
    (0..b.len().saturating_sub(3)).any(|i| {
        if !b[i..i + 4].iter().all(u8::is_ascii_digit) {
            return false;
        }
        if i > 0 && (b[i - 1] as char).is_ascii_digit() {
            return false;
        }
        if i + 4 < b.len() && (b[i + 4] as char).is_ascii_digit() {
            return false;
        }
        let y: i64 = text[i..i + 4].parse().unwrap_or(0);
        (1900..=2100).contains(&y)
    })
}

/// A BARE day-and-month resolves within twelve months of today.
///
/// SYN-204. "on s'est mariés le 12 juin" carries no year, so the only years
/// the capture can mean are the most recent 12 June already gone and the next
/// one to come. A resolution thirteen months back is wrong whatever the tense,
/// and the model produced exactly that.
///
/// The DIRECTION is left to the model, because the direction belongs to the
/// tense and deciding it here would decide in the rule's place: a date it put
/// in the past stays in the past, one it put ahead stays ahead. Only the YEAR
/// moves, and only when the capture states none. Nothing is restricted: a date
/// already inside the window, or one whose capture writes a year, comes back
/// untouched.
fn snap_bare_day_month(date: &str, capture: &str, today: &str) -> String {
    if date.len() < 10 || today.len() < 10 || states_a_year(capture) {
        return date.to_string();
    }
    let head = &date[0..10];
    let (Ok(m), Ok(d)) = (head[5..7].parse::<i64>(), head[8..10].parse::<i64>()) else {
        return date.to_string();
    };
    let Ok(ty) = today[0..4].parse::<i64>() else {
        return date.to_string();
    };
    // The two candidates the capture can mean, and nothing else: the most
    // recent occurrence already gone, and the next one to come.
    let this_year = format!("{ty:04}-{m:02}-{d:02}");
    let gone = if this_year.as_str() > today { ty - 1 } else { ty };
    let want = if head < today { gone } else { gone + 1 };
    if head[0..4].parse::<i64>() == Ok(want) {
        return date.to_string();
    }
    // 29 February only exists on a leap year: re-anchoring it would produce a
    // date chrono refuses to parse, which costs the notification outright.
    if d > month_len(want, m) {
        return date.to_string();
    }
    format!("{want:04}-{m:02}-{d:02}{}", &date[10..])
}

/// A date resolved from a NAMED weekday must FALL on that weekday.
///
/// SYN-204 / SYN-213. This is the one date invariant that needs no arbitration:
/// whatever the tense, whatever the direction, "jeudi" is a Thursday. When the
/// model writes a date that bears another day's name it is wrong on its own
/// terms, and we snap it to the nearest date carrying the named day, keeping
/// the DIRECTION the model chose relative to today. Its reading of past versus
/// future is left alone: that reading belongs to the tense, and overriding it
/// here would decide in the rule's place.
///
/// It NEVER fires on a date that is already right, so it restricts nothing. It
/// stays silent when the capture names no weekday, or names two.
///
/// The same check runs on the corpus side (`scripts/parity/etiqueter.py`), on
/// the labelling model, which makes the same mistake in the same direction.
fn snap_to_named_weekday(date: &str, capture: &str, today: &str) -> String {
    if date.len() < 10 {
        return date.to_string();
    }
    let head = &date[0..10];
    let Some(want) = named_weekday(capture) else {
        return date.to_string();
    };
    let Some(got) = weekday_index(head) else {
        return date.to_string();
    };
    if got == want {
        return date.to_string();
    }
    // The NEAREST date bearing the name, at most three days away. Moving
    // further would rewrite the model's reading of the date rather than fix
    // the day it names.
    let forward = ((want + 7 - got) % 7) as i64;
    let delta = if forward <= 3 { forward } else { forward - 7 };
    let mut snapped = add_days_iso(head, delta);
    // ...but never across today. The model's reading of past versus future
    // belongs to the tense, and a snap of one or two days must not flip it:
    // a Friday it placed before today stays before today.
    if (head < today) != (snapped.as_str() < today) {
        snapped = add_days_iso(head, if delta > 0 { delta - 7 } else { delta + 7 });
    }
    // Keep any time suffix the model wrote after the date.
    format!("{snapped}{}", &date[10..])
}

/// Day arithmetic on an ISO `YYYY-MM-DD` string (Gregorian, no deps).
fn add_days_iso(date: &str, days: i64) -> String {
    let (mut y, mut m, mut d) = (
        date[0..4].parse::<i64>().unwrap_or(1970),
        date[5..7].parse::<i64>().unwrap_or(1),
        date[8..10].parse::<i64>().unwrap_or(1),
    );
    d += days;
    while d > month_len(y, m) {
        d -= month_len(y, m);
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    while d < 1 {
        m -= 1;
        if m < 1 {
            m = 12;
            y -= 1;
        }
        d += month_len(y, m);
    }
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;


    // ── SYN-204 : une date nue jour+mois tient dans douze mois ─────────────

    #[test]
    fn a_written_year_is_left_alone() {
        assert!(states_a_year("on s'est mariés le 12 juin 2019"));
        assert!(states_a_year("born in 1990"));
        assert!(!states_a_year("on s'est mariés le 12 juin"));
        // Un nombre qui n'est pas une année n'en est pas une.
        assert!(!states_a_year("budget 150000 euros"));
        assert!(!states_a_year("on en a dépensé 87k"));
    }

    #[test]
    fn bare_day_month_comes_back_inside_the_window() {
        let today = "2026-07-13";
        // Le défaut mesuré : le 12 juin déjà passé de cette année, pas celui
        // de l'année d'avant.
        assert_eq!(
            snap_bare_day_month("2025-06-12", "on s'est mariés le 12 juin", today),
            "2026-06-12"
        );
        // Vers l'avant, la direction du modèle est respectée.
        assert_eq!(
            snap_bare_day_month("2027-08-03", "le forum est le 3 août", today),
            "2026-08-03"
        );
    }

    #[test]
    fn bare_day_month_restricts_nothing() {
        let today = "2026-07-13";
        // Déjà dans la fenêtre : intacte, des deux côtés.
        assert_eq!(
            snap_bare_day_month("2026-06-12", "le 12 juin", today),
            "2026-06-12"
        );
        assert_eq!(
            snap_bare_day_month("2026-08-03", "le 3 août", today),
            "2026-08-03"
        );
        // Une année ÉCRITE dans la capture : on n'y touche pas, même loin.
        assert_eq!(
            snap_bare_day_month("2019-06-12", "on s'est mariés le 12 juin 2019", today),
            "2019-06-12"
        );
        // Une naissance porte son année et échappe donc à la fenêtre.
        assert_eq!(
            snap_bare_day_month("1990-03-03", "né le 3 mars 1990", today),
            "1990-03-03"
        );
        // Le 29 février ne se réancre pas sur une année non bissextile.
        assert_eq!(
            snap_bare_day_month("2024-02-29", "le 29 février", today),
            "2024-02-29"
        );
        // L'heure écrite après la date survit.
        assert_eq!(
            snap_bare_day_month("2025-06-12T18:00", "le 12 juin", today),
            "2026-06-12T18:00"
        );
    }

    // ── SYN-213 : une date issue d'un jour NOMMÉ tombe sur ce jour ──────────

    #[test]
    fn weekday_index_reads_the_calendar() {
        // 13 juillet 2026 est un lundi, le temps de référence du harnais.
        assert_eq!(weekday_index("2026-07-13"), Some(0));
        assert_eq!(weekday_index("2026-07-09"), Some(3)); // jeudi
        assert_eq!(weekday_index("2026-07-10"), Some(4)); // vendredi
        assert_eq!(weekday_index("2026-03-01"), Some(6)); // dimanche, mois < 3
        assert_eq!(weekday_index("2024-02-29"), Some(3)); // bissextile
        assert_eq!(weekday_index("2026-02-30"), None); // n'existe pas
        assert_eq!(weekday_index("2026-07"), None);
    }

    #[test]
    fn named_weekday_needs_exactly_one_and_a_whole_word() {
        assert_eq!(named_weekday("On a mangé des pâtes jeudi soir"), Some(3));
        assert_eq!(named_weekday("Design review on Friday"), Some(4));
        // Deux jours nommés : rien ne dit auquel la date appartient.
        assert_eq!(named_weekday("on s'est vus mardi puis jeudi"), None);
        // Le même jour deux fois reste un seul jour.
        assert_eq!(named_weekday("jeudi, oui jeudi"), Some(3));
        // Pas de faux positif à l'intérieur d'un mot.
        assert_eq!(named_weekday("je suis allé au marché"), None);
        assert_eq!(named_weekday("it was a sunny morning"), None);
        assert_eq!(named_weekday("rien de temporel ici"), None);
    }

    #[test]
    fn snap_fixes_the_day_and_keeps_the_direction() {
        let today = "2026-07-13"; // lundi
        // Le défaut mesuré : « jeudi » au passé sortait au 10, un vendredi.
        assert_eq!(
            snap_to_named_weekday("2026-07-10", "On a mangé des pâtes jeudi soir", today),
            "2026-07-09"
        );
        // Vers l'avant, la direction du modèle est respectée.
        assert_eq!(
            snap_to_named_weekday("2026-07-17", "On mange des pâtes jeudi soir", today),
            "2026-07-16"
        );
        // L'heure écrite après la date survit.
        assert_eq!(
            snap_to_named_weekday("2026-07-10T20:00", "des pâtes jeudi soir", today),
            "2026-07-09T20:00"
        );
        // Mesuré le 28/08 sur le harnais : « avant vendredi » sortait au 18, un samedi.
        // Le harnais n'exécute pas ce fichier, il continuera de l'afficher en écart.
        assert_eq!(
            snap_to_named_weekday(
                "2026-07-18",
                "faut que j'envoie les papiers à la CAF avant vendredi",
                today
            ),
            "2026-07-17"
        );
    }

    #[test]
    fn snap_restricts_nothing() {
        let today = "2026-07-13";
        // Déjà juste : intacte.
        assert_eq!(
            snap_to_named_weekday("2026-07-09", "on a mangé jeudi", today),
            "2026-07-09"
        );
        // Aucun jour nommé : intacte, même si la date est loin.
        assert_eq!(
            snap_to_named_weekday("2026-06-12", "on s'est mariés le 12 juin", today),
            "2026-06-12"
        );
        // Deux jours nommés : intacte, on ne devine pas.
        assert_eq!(
            snap_to_named_weekday("2026-07-10", "vus mardi puis jeudi", today),
            "2026-07-10"
        );
        // Date inexploitable : intacte.
        assert_eq!(snap_to_named_weekday("jeudi", "jeudi", today), "jeudi");
    }

    // ── SYN-190 ────────────────────────────────────────────────────────────

    #[test]
    fn signature_collapses_empty_affixes_and_tense() {
        let sig = Brain::predicate_signature;
        assert_eq!(sig("is_cousin_of"), sig("cousin_of"));
        assert_eq!(sig("worked_at"), sig("works_at"));
        assert_eq!(sig("has_birthday"), sig("birthday"));
        // Et ce qu'elle NE fusionne PAS : deux prépositions différentes font deux
        // affirmations différentes, et c'est justement le cas que l'embedding doit
        // rattraper (`works_as` → `works_at`), pas la signature.
        assert_ne!(sig("works_as"), sig("works_at"));
        // Un stemmer trop gourmand casserait celui-ci.
        assert_ne!(sig("born_on"), sig("borrows"));
    }

    #[test]
    fn sibling_pairs_are_never_a_merge() {
        // Un mot qui diffère à la MÊME place : une affirmation, deux valeurs.
        // Les fusionner détruirait l'information, et c'est pourtant la paire que
        // l'embedding note le plus haut (0,92 mesuré le 2026-08-24).
        assert!(Brain::is_sibling_pair(
            "supports_manual_tagging",
            "supports_automatic_tagging"
        ));
        assert!(Brain::is_sibling_pair(
            "is_primary_channel_for",
            "is_secondary_channel_for"
        ));
        // Deux vrais synonymes ne sont PAS des jumeaux : ils doivent passer.
        assert!(!Brain::is_sibling_pair("works_as", "works_at"));
        assert!(!Brain::is_sibling_pair("profession", "job_title"));
    }

    #[test]
    fn proposal_points_toward_the_family_head() {
        // La direction décide si accepter la proposition RÉPARE le supersede ou
        // le casse pour de bon. `works_at` est tête de famille, `works_as` non :
        // la cible doit être la tête, quel que soit l'ordre d'appel.
        // Table créée à la main : `init_schema` monte tout le schéma, y compris
        // les tables virtuelles vec0 que ce test n'a pas et dont il n'a pas besoin.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE predicate_merge_proposals (
                id TEXT PRIMARY KEY, kind TEXT, candidate_predicate TEXT,
                existing_predicate TEXT, similarity_score REAL,
                similarity_reason TEXT, evidence_capture_id TEXT,
                status TEXT DEFAULT 'pending')",
        )
        .unwrap();
        record_predicate_proposal(&conn, "fact", "works_at", "works_as", 0.93, "t", "c1")
            .unwrap();
        let (cand, exist): (String, String) = conn
            .query_row(
                "SELECT candidate_predicate, existing_predicate \
                 FROM predicate_merge_proposals",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((cand.as_str(), exist.as_str()), ("works_as", "works_at"));

        // Idempotent dans les deux sens : la même paire ne fait pas deux lignes.
        record_predicate_proposal(&conn, "fact", "works_as", "works_at", 0.93, "t", "c2")
            .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM predicate_merge_proposals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn confidence_matches_python_formula() {
        // explicit, new entity, mention 1, persistence 3: 0.92 + 0.02 + 0.05
        assert_eq!(compute_confidence(3, "explicit", false, 1), 0.99);
        // hedged clamps under the facts threshold whatever the bonuses
        assert_eq!(compute_confidence(5, "hedged", true, 9), 0.84);
        // implicit low
        assert!((compute_confidence(1, "implicit", false, 1) - 0.32).abs() < 1e-12);
        // cap at 1.0
        assert_eq!(compute_confidence(5, "explicit", true, 9), 1.0);
    }

    #[test]
    fn date_resolution_covers_recorded_shapes() {
        assert_eq!(resolve_date("2026-06-16", "2026-07-04"), "2026-06-16");
        assert_eq!(resolve_date("1993", "2026-07-04"), "1993-07-01");
        assert_eq!(resolve_date("07-04", "2026-07-04"), "2026-07-04");
        assert_eq!(resolve_date("next week", "2026-07-04"), "2026-07-11");
        assert_eq!(resolve_date("bientôt", "2026-07-04"), "bientôt");
        assert_eq!(add_days_iso("2026-12-28", 7), "2027-01-04");
    }

    /// A month name left unresolved is not cosmetic: `digest::next_occurrence`
    /// parses ISO strictly, so "June 12" on a fiche is a birthday that never
    /// reaches a notification.
    #[test]
    fn month_names_resolve_in_both_languages() {
        let today = "2026-08-20";
        assert_eq!(resolve_date("12 juin", today), "2026-06-12");
        assert_eq!(resolve_date("June 12", today), "2026-06-12");
        assert_eq!(resolve_date("3 mars 1990", today), "1990-03-03");
        assert_eq!(resolve_date("March 3, 1990", today), "1990-03-03");
        assert_eq!(resolve_date("le 1er août", today), "2026-08-01");
        assert_eq!(resolve_date("décembre 25", today), "2026-12-25");
        // Out of range, unparseable, or ambiguous → left exactly as written.
        assert_eq!(resolve_date("31 février", today), "31 février");
        assert_eq!(resolve_date("un jour de juin", today), "un jour de juin");
        assert_eq!(resolve_date("entre mars et avril", today), "entre mars et avril");
    }

    /// Asked on 20 August about a birthday "le 23 juillet", the model answers
    /// with next year's occurrence — which stored as a birth year says the
    /// person will be born in 2027.
    #[test]
    fn a_birth_date_is_never_in_the_future() {
        let today = "2026-08-20";
        assert_eq!(resolve_fact_date("2027-07-23", "has_birthday", today, "son anniversaire est le 23 juillet"), "2026-07-23");
        // Still ahead after re-anchoring to this year → the year before.
        assert_eq!(resolve_fact_date("2027-12-25", "birthday", today, "son anniversaire est le 25 décembre"), "2025-12-25");
        // A real birth year is a claim, not an anchor: it stays.
        assert_eq!(resolve_fact_date("1990-03-03", "has_birthday", today, "né le 3 mars 1990"), "1990-03-03");
        // 29 February survives only on a leap year — moving it would make the
        // date unparseable, which costs the notification outright.
        assert_eq!(resolve_fact_date("2028-02-29", "has_birthday", today, "son anniversaire est le 29 février"), "2028-02-29");
        // A deadline or a next appointment is legitimately ahead of us.
        assert_eq!(resolve_fact_date("2027-07-23", "next_meeting_date", today, "prochaine réunion le 23 juillet 2027"), "2027-07-23");
    }

    #[test]
    fn single_valued_predicates_group_by_the_claim_they_make() {
        let birthday = single_valued_family("has_birthday").unwrap();
        assert!(birthday.contains(&"birthday"));
        assert!(birthday.contains(&"date_of_birth"));
        // Same family whichever synonym the model reached for, and casing or
        // stray whitespace must not open a second lane.
        assert_eq!(single_valued_family(" Birthday "), Some(birthday));
        // Multi-valued predicates keep accumulating.
        assert!(single_valued_family("likes").is_none());
        // Distinct claims stay distinct: a phone must not supersede an email.
        assert_ne!(single_valued_family("phone"), single_valued_family("email"));
    }

    #[test]
    fn py_dumps_matches_python_layout() {
        let v: Value = serde_json::from_str(r#"{"b": 1, "a": ["x", 2], "c": "é"}"#).unwrap();
        assert_eq!(py_dumps(&v), r#"{"b": 1, "a": ["x", 2], "c": "é"}"#);
        assert_eq!(py_dumps_ascii(&v), r#"{"b": 1, "a": ["x", 2], "c": "\u00e9"}"#);
    }

    #[test]
    fn truthiness_is_python_truthiness() {
        assert!(!truthy(Some(&json!([]))));
        assert!(!truthy(Some(&json!(""))));
        assert!(!truthy(Some(&json!(0))));
        assert!(!truthy(Some(&json!(null))));
        assert!(truthy(Some(&json!([1]))));
        assert!(truthy(Some(&json!(false))) == false);
    }

    // SYN-119 — the language the classifier detected must flow end-to-end:
    // ── Les ressources ──────────────────────────────────────────────────

    fn capture_lien(entites: Value, ressources: Value, note: Value) -> Value {
        json!({
            "language": "fr",
            "atomic_note": note,
            "atomic_note_kind": if note.is_null() { "note" } else { "note" },
            "is_ephemeral": false,
            "entities": entites,
            "relations": [],
            "project_entries": [],
            "resources": ressources,
            "classification_confidence": 1.0
        })
    }

    fn une(brain: &Brain, sql: &str) -> Vec<String> {
        let conn = brain.storage.lock().unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((0..r.as_ref().column_count())
                    .map(|i| match r.get_ref(i).unwrap() {
                        rusqlite::types::ValueRef::Null => "∅".to_string(),
                        rusqlite::types::ValueRef::Text(t) => {
                            String::from_utf8_lossy(t).to_string()
                        }
                        v => format!("{v:?}"),
                    })
                    .collect::<Vec<_>>()
                    .join(" | "))
            })
            .unwrap();
        rows.collect::<Result<Vec<_>, _>>().unwrap()
    }

    #[test]
    fn un_lien_vers_une_chose_qui_a_son_identite_se_pose_sur_elle() {
        // « le tableau Linear https://linear.app/… » : Linear existe déjà comme
        // outil. Une seconde fiche « ressource » à côté serait le doublon que
        // la file de fusion existe pour nettoyer.
        let brain = router_entite(capture_lien(
            json!([{"canonical_name": "Linear", "type": "tool", "aliases": [],
                    "summary": null, "attributes": {}, "facts": []}]),
            json!([{"url": "https://linear.app/board", "category": "page",
                    "entity_canonical": "Linear", "user_comment": null}]),
            json!("Le tableau Linear"),
        ));
        assert_eq!(
            une(&brain, "SELECT canonical_name, type FROM entities"),
            vec!["Linear | tool"],
            "une seule fiche, celle de l'outil"
        );
        assert_eq!(
            une(&brain, "SELECT attributes FROM entities WHERE canonical_name = 'Linear'"),
            vec![r#"{"url": "https://linear.app/board", "resource_category": "page"}"#]
        );
    }

    #[test]
    fn un_lien_qui_EST_la_chose_devient_une_fiche_ressource() {
        let brain = router_entite(capture_lien(
            json!([{"canonical_name": "Un article sur la mémoire", "type": "resource",
                    "aliases": [], "summary": null, "attributes": {}, "facts": []}]),
            json!([{"url": "https://example.com/article", "category": "article",
                    "entity_canonical": "Un article sur la mémoire",
                    "user_comment": "super intéressant sur la mémoire"}]),
            json!("Un article super intéressant sur la mémoire"),
        ));
        assert_eq!(
            une(&brain, "SELECT canonical_name, type FROM entities"),
            vec!["Un article sur la mémoire | resource"]
        );
        // Le commentaire de l'auteur devient le résumé : il dit pourquoi LUI
        // l'a gardé, ce qu'aucun résumé de la page ne sait dire.
        assert_eq!(
            une(&brain, "SELECT summary FROM entities"),
            vec!["super intéressant sur la mémoire"]
        );
        assert_eq!(
            une(&brain, "SELECT url, type, user_comment FROM resources"),
            vec!["https://example.com/article | article | super intéressant sur la mémoire"]
        );
    }

    #[test]
    fn la_fiche_qui_porte_un_lien_ne_passe_pas_par_la_file_de_creation() {
        // Aucun fait, aucun lien : sans exemption elle serait proposée. Le
        // geste de garder l'URL EST la preuve.
        let brain = router_entite(capture_lien(
            json!([{"canonical_name": "Un article", "type": "resource", "aliases": [],
                    "summary": null, "attributes": {}, "facts": []}]),
            json!([{"url": "https://example.com/a", "category": "article",
                    "entity_canonical": "Un article", "user_comment": "à lire"}]),
            json!("Un article à lire"),
        ));
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entities"), 1);
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entity_creation_proposals"), 0);
    }

    #[test]
    fn le_lien_nexempte_que_la_fiche_quil_porte() {
        // « Ryusuke Hamaguchi https://share.google/… » : la ressource naît, la
        // personne reste soumise à la file. Sinon un lien serait un passe-droit
        // pour tout ce qui traîne dans la même phrase.
        let classified = json!({
            "language": "fr",
            "atomic_note": "Ryusuke Hamaguchi",
            "atomic_note_kind": "task",
            "is_ephemeral": false,
            "entities": [
                {"canonical_name": "Ryusuke Hamaguchi", "type": "person", "aliases": [],
                 "summary": null, "attributes": {}, "facts": []},
                {"canonical_name": "Une page sur Hamaguchi", "type": "resource",
                 "aliases": [], "summary": null, "attributes": {}, "facts": []}
            ],
            "relations": [],
            "project_entries": [],
            "resources": [{"url": "https://share.google/x", "category": "page",
                           "entity_canonical": "Une page sur Hamaguchi",
                           "user_comment": null}],
            "classification_confidence": 1.0
        });
        let brain = router_entite(classified);
        assert_eq!(
            une(&brain, "SELECT canonical_name FROM entities"),
            vec!["Une page sur Hamaguchi"]
        );
        assert_eq!(
            une(&brain, "SELECT canonical_name FROM entity_creation_proposals"),
            vec!["Ryusuke Hamaguchi"]
        );
    }

    #[test]
    fn le_meme_lien_deux_fois_ne_fait_quune_ligne() {
        let c = capture_lien(
            json!([{"canonical_name": "Un article", "type": "resource", "aliases": [],
                    "summary": null, "attributes": {}, "facts": []}]),
            json!([{"url": "https://example.com/a", "category": "article",
                    "entity_canonical": "Un article", "user_comment": "à lire"}]),
            json!("Un article à lire"),
        );
        let brain = router_entite(c.clone());
        {
            let conn = brain.storage.lock().unwrap();
            conn.execute("INSERT INTO inbox (id, content) VALUES ('c2', 'x')", [])
                .unwrap();
        }
        let ctx = RouteContext {
            now: "2026-07-14T12:00:00".into(),
            today: "2026-07-14".into(),
            intentions_cutoff: "2026-07-12T12:00:00".into(),
            now_sql: "2026-07-14 12:00:00".into(),
        };
        brain
            .route_capture(&json!({"id": "c2", "content": "x"}), &c, &ctx)
            .unwrap();
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM resources"), 1);
    }

    // ── Annuler une action retire la tâche déjà enregistrée ─────────────

    fn hit(id: &str, score: f64) -> TaskHit {
        TaskHit { note_id: id.into(), content: format!("tâche {id}"), score }
    }

    #[test]
    fn une_cible_nette_est_archivee() {
        // Assez proche, et largement détachée de la suivante.
        assert_eq!(
            decide_cancellation(&[hit("t1", 0.81), hit("t2", 0.40)], 0.62, 0.08),
            CancelDecision::Archive("t1".into())
        );
        // Seule candidate : rien avec quoi la confondre.
        assert_eq!(
            decide_cancellation(&[hit("t1", 0.63)], 0.62, 0.08),
            CancelDecision::Archive("t1".into())
        );
    }

    #[test]
    fn deux_taches_au_coude_a_coude_ne_se_tranchent_pas_seules() {
        // Le cas qui coûte le plus cher : deux tâches se ressemblent autant.
        // Archiver la mauvaise sort du backlog, et un backlog est une liste
        // qu'on lit pour savoir ce qui RESTE — personne n'y cherche ce qui
        // n'y est plus.
        let d = decide_cancellation(&[hit("t1", 0.80), hit("t2", 0.76)], 0.62, 0.08);
        assert_eq!(
            d,
            CancelDecision::Ask { reason: "ambigu", candidates: vec!["t1".into(), "t2".into()] }
        );
    }

    #[test]
    fn une_ressemblance_lointaine_se_demande_au_lieu_de_sappliquer() {
        let d = decide_cancellation(&[hit("t1", 0.45), hit("t2", 0.20)], 0.62, 0.08);
        // Le motif dit LAQUELLE des deux incertitudes : rien ne ressemble
        // vraiment à ce qui est annulé.
        assert_eq!(
            d,
            CancelDecision::Ask { reason: "approximatif", candidates: vec!["t1".into()] }
        );
        // Et la candidate montrée descend sous le seuil d'action : la question
        // n'engage rien, contrairement à l'archivage.
        let d = decide_cancellation(&[hit("t1", 0.10)], 0.62, 0.08);
        assert_eq!(
            d,
            CancelDecision::Ask { reason: "approximatif", candidates: vec!["t1".into()] }
        );
    }

    #[test]
    fn renoncer_est_une_decision_et_la_decision_se_garde() {
        // Le modèle laisse tomber la note dès qu'il remplit le pointeur :
        // mesuré 3 fois sur 3, sur trois captures, avec quatre formulations du
        // prompt. Le code la rétablit, et le contenu brut EST la décision.
        let mut c = abandon();
        c["cancels_action"] = json!("la réservation du gîte");
        let capture = "euh non finalement laisse tomber la réservation du gîte on part plus";
        let brain = router_contenu(capture, c);
        let conn = brain.storage.lock().unwrap();
        let (contenu, kind, statut): (String, String, String) = conn
            .query_row(
                "SELECT content, kind, review_status FROM atomic_notes",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(contenu, capture);
        assert_eq!(kind, "note");
        // `confirmed` : c'est la CIBLE qui peut être douteuse, jamais le fait
        // que l'auteur a renoncé.
        assert_eq!(statut, "confirmed");
    }

    #[test]
    fn la_note_du_modele_gagne_quand_il_en_ecrit_une() {
        // Le repêchage ne doit pas doubler la note quand le modèle a fait son
        // travail : une capture, un souvenir.
        let mut c = abandon();
        c["atomic_note"] = json!("Finalement je n'envoie pas le devis à Acme");
        c["atomic_note_kind"] = json!("note");
        c["cancels_action"] = json!("envoyer le devis à Acme");
        let brain = router_contenu("Finalement je n'envoie pas le devis à Acme", c);
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM atomic_notes"), 1);
        let conn = brain.storage.lock().unwrap();
        let contenu: String = conn
            .query_row("SELECT content FROM atomic_notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(contenu, "Finalement je n'envoie pas le devis à Acme");
    }

    #[test]
    fn sans_embarqueur_on_ne_devine_pas() {
        // Le cerveau des tests n'en a pas. La décision de renoncer est écrite
        // comme d'habitude — rien n'est perdu — mais aucune tâche ne part et
        // aucune question n'est posée sur une ressemblance qu'on n'a pas pu
        // mesurer.
        let mut c = abandon();
        c["atomic_note"] = json!("Je ne vais finalement pas appeler le dentiste");
        c["atomic_note_kind"] = json!("note");
        c["cancels_action"] = json!("appeler le dentiste");
        let brain = router_contenu("je ne vais finalement pas appeler le dentiste", c);
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM note_cancellation_proposals"), 0);
        assert_eq!(
            compte(&brain, "SELECT COUNT(*) FROM atomic_notes WHERE archived_at IS NOT NULL"),
            0
        );
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM atomic_notes"), 1);
    }

    // ── Une capture qui n'a rien laissé ─────────────────────────────────

    /// Comme `router_entite`, mais le CONTENU brut compte ici : c'est lui qu'on
    /// retrouve en file quand la capture n'a rien laissé d'autre.
    fn router_contenu(contenu: &str, classified: Value) -> Brain {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.keep().join("s.db");
        let brain = Brain::open(db.to_str().unwrap(), None).unwrap();
        {
            let conn = brain.storage.lock().unwrap();
            conn.execute("INSERT INTO inbox (id, content) VALUES ('c1', ?1)", [contenu])
                .unwrap();
        }
        let ctx = RouteContext {
            now: "2026-07-13T12:00:00".into(),
            today: "2026-07-13".into(),
            intentions_cutoff: "2026-07-11T12:00:00".into(),
            now_sql: "2026-07-13 12:00:00".into(),
        };
        brain
            .route_capture(&json!({"id": "c1", "content": contenu}), &classified, &ctx)
            .unwrap();
        brain
    }

    /// Le squelette d'un abandon : le modèle a tout mis à null et il est sûr.
    fn abandon() -> Value {
        json!({
            "language": "fr",
            "atomic_note": null,
            "atomic_note_kind": null,
            "event_date": null,
            "is_ephemeral": false,
            "entities": [],
            "relations": [],
            "project_entries": [],
            "obsoleted_facts": [],
            "resources": [],
            "classification_confidence": 1.0
        })
    }

    #[test]
    fn une_capture_qui_ne_laisse_rien_atteint_la_file() {
        // Arbitré : « J'ai lavé la voiture hier » ne doit RIEN garder, mais on
        // doit demander. Le modèle rend 1,0 et il a raison — le routage est
        // évident. C'est l'abandon qu'on relit, et il ne se lit pas dans la
        // confiance : sans note, aucune ligne n'existait où écrire un statut.
        let brain = router_contenu("J'ai lavé la voiture hier", abandon());
        assert_eq!(
            compte(&brain, "SELECT COUNT(*) FROM atomic_notes \
                            WHERE review_status = 'pending' AND review_reason = 'rien_garde'"),
            1
        );
        // Le contenu brut, tel qu'écrit : c'est tout ce qui reste de la capture.
        let conn = brain.storage.lock().unwrap();
        let contenu: String = conn
            .query_row("SELECT content FROM atomic_notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(contenu, "J'ai lavé la voiture hier");
    }

    #[test]
    fn une_fiche_vide_nest_pas_une_trace() {
        // « Léa : … » fabriquait une fiche Léa sans le moindre fait et perdait
        // tout le reste. Un nom seul n'apprend rien : la capture part en file.
        let mut c = abandon();
        c["entities"] = json!([{
            "canonical_name": "Léa", "type": "person", "aliases": [], "facts": []
        }]);
        let brain = router_contenu("Léa : changer les serrures", c);
        assert_eq!(
            compte(&brain, "SELECT COUNT(*) FROM atomic_notes \
                            WHERE review_reason = 'rien_garde'"),
            1
        );
    }

    #[test]
    fn le_rien_garde_ne_restreint_rien() {
        // Quatre captures qui ont laissé quelque chose. Aucune ne doit être
        // remise en question : une file noyée se clique sans se lire.
        let mut avec_note = abandon();
        avec_note["atomic_note"] = json!("Penser à rappeler le notaire");
        avec_note["atomic_note_kind"] = json!("task");

        let mut avec_fait = abandon();
        avec_fait["entities"] = json!([{
            "canonical_name": "Pierre", "type": "person", "aliases": [],
            "facts": [{"predicate": "works_at", "value": "Acme",
                       "persistence_value": 4, "evidence_strength": "explicit",
                       "category": "work"}]
        }]);

        let mut avec_projet = abandon();
        avec_projet["project_entries"] =
            json!([{"project_canonical": "rénovation", "content": "posé les rails",
                    "is_new": true}]);

        let mut avec_lien = abandon();
        avec_lien["resources"] = json!([{"url": "https://example.org/a", "title": "a"}]);

        for (nom, c) in [
            ("note", avec_note),
            ("fait", avec_fait),
            ("projet", avec_projet),
            ("lien", avec_lien),
        ] {
            let brain = router_contenu("peu importe", c);
            assert_eq!(
                compte(&brain, "SELECT COUNT(*) FROM atomic_notes \
                                WHERE review_reason = 'rien_garde'"),
                0,
                "une capture qui a laissé un {nom} n'a rien à faire en file"
            );
        }
    }

    // ── Entité proposée plutôt que créée ────────────────────────────────

    /// Un cerveau, une capture en boîte, et la capture routée. Rend le cerveau
    /// pour que chaque test dise ensuite ce qu'il vérifie, sans répéter le
    /// montage.
    fn router_entite(classified: Value) -> Brain {
        let dir = tempfile::tempdir().unwrap();
        // Le dossier temporaire doit survivre au retour : on le laisse fuir
        // exprès, le test étant plus court que le processus.
        let db = dir.keep().join("s.db");
        let brain = Brain::open(db.to_str().unwrap(), None).unwrap();
        {
            let conn = brain.storage.lock().unwrap();
            conn.execute("INSERT INTO inbox (id, content) VALUES ('c1', 'x')", [])
                .unwrap();
        }
        let ctx = RouteContext {
            now: "2026-07-13T12:00:00".into(),
            today: "2026-07-13".into(),
            intentions_cutoff: "2026-07-11T12:00:00".into(),
            now_sql: "2026-07-13 12:00:00".into(),
        };
        brain
            .route_capture(&json!({"id": "c1", "content": "x"}), &classified, &ctx)
            .unwrap();
        brain
    }

    fn capture_fete(persistance: i64) -> Value {
        json!({
            "language": "fr",
            "atomic_note": "J'ai la fête de Pierre le 20",
            "atomic_note_kind": "event",
            "event_date": "2026-07-20",
            "is_ephemeral": false,
            "entities": [{
                "canonical_name": "Pierre",
                "type": "person",
                "aliases": [],
                "facts": [{
                    "predicate": "attends", "value": "fête",
                    "persistence_value": persistance,
                    "evidence_strength": "explicit", "category": "social"
                }]
            }],
            "relations": [],
            "project_entries": [],
            "classification_confidence": 1.0
        })
    }

    fn compte(brain: &Brain, sql: &str) -> i64 {
        let conn = brain.storage.lock().unwrap();
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn une_entite_seulement_nommee_est_proposee_et_non_creee() {
        let brain = router_entite(capture_fete(1));
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entities"), 0,
                   "« J'ai la fête de Pierre le 20 » ne doit plus fabriquer de fiche");
        assert_eq!(
            compte(&brain, "SELECT COUNT(*) FROM entity_creation_proposals \
                            WHERE canonical_name = 'Pierre' AND status = 'pending'"),
            1
        );
        // La note, elle, est écrite : l'entité attend, la capture non. Sans ça
        // on remplacerait une fiche de trop par une capture perdue.
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM atomic_notes"), 1);
    }

    /// Une capture qui ne laisse qu'un ÉPISODE, avec une entité que rien
    /// d'autre ne porte. C'est « J'étais seul à la Bibliothèque Forney hier ».
    fn capture_lieu_episode(persistance: i64) -> Value {
        let mut facts = vec![];
        if persistance > 0 {
            facts.push(json!({
                "predicate": "visited", "value": "après-midi",
                "persistence_value": persistance,
                "evidence_strength": "explicit", "category": "places"
            }));
        }
        json!({
            "language": "fr",
            "atomic_note": "J'étais seul à la Bibliothèque Forney hier",
            "atomic_note_kind": "episode",
            "is_ephemeral": false,
            "entities": [{
                "canonical_name": "Bibliothèque Forney", "type": "place",
                "aliases": [], "facts": facts
            }],
            "relations": [],
            "project_entries": [],
            "classification_confidence": 1.0
        })
    }

    #[test]
    fn un_lieu_dun_episode_est_propose_et_non_ignore() {
        // Avant : `durable_note` ne valait que pour `task` et `event`, donc un
        // épisode n'ancrait rien, donc la fiche n'était ni créée NI proposée.
        // Elle disparaissait sans question. Or un épisode asserte que quelque
        // chose a eu lieu : il ancre autant qu'une tâche.
        let brain = router_entite(capture_lieu_episode(0));
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entities"), 0);
        assert_eq!(
            compte(&brain, "SELECT COUNT(*) FROM entity_creation_proposals \
                            WHERE canonical_name = 'Bibliothèque Forney' \
                            AND status = 'pending'"),
            1
        );
        // Et la note de l'épisode est écrite quoi qu'il arrive : c'est la
        // fiche qui attend, pas la capture.
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM atomic_notes"), 1);
    }

    #[test]
    fn une_mention_unique_sans_detail_ne_fabrique_plus_de_fiche() {
        // « Vivatech c'est le 24 » : un seul fait, de persistance 3, qui n'est
        // que la date redite. Ça passait le plancher de 2 et créait la fiche.
        let brain = router_entite(capture_fete(3));
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entities"), 0);
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entity_creation_proposals"), 1);
    }

    #[test]
    fn une_date_redite_ne_prouve_rien_quelle_que_soit_sa_persistance() {
        // « Vivatech c'est le 24 » : le seul fait est la date de l'occurrence,
        // qui ne dit rien de l'entité au-delà du jour où elle a lieu. Le
        // modèle lui donne 3 ou 4 selon la passe, mesuré le 28/08, donc un
        // palier chiffré tomberait pile sur la bascule.
        for persistance in [3, 4, 5] {
            let mut c = capture_fete(persistance);
            c["entities"][0]["facts"] = json!([{
                "predicate": "event_date", "value": "2026-07-24",
                "persistence_value": persistance,
                "evidence_strength": "explicit", "category": "events"
            }]);
            let brain = router_entite(c);
            assert_eq!(
                compte(&brain, "SELECT COUNT(*) FROM entities"),
                0,
                "persistance {persistance} : une date redite ne fait pas naître de fiche"
            );
            assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entity_creation_proposals"), 1);
        }
    }

    #[test]
    fn deux_faits_suffisent_encore_a_faire_naitre_une_fiche() {
        // Le palier ne monte QUE pour une entité seule au monde. Deux faits,
        // et le plancher habituel s'applique : sans ça on assécherait le
        // graphe, qui naît surtout de cette clause.
        let mut c = capture_fete(2);
        c["entities"][0]["facts"] = json!([
            {"predicate": "attends", "value": "fête", "persistence_value": 2,
             "evidence_strength": "explicit", "category": "social"},
            {"predicate": "lives_in", "value": "Lyon", "persistence_value": 2,
             "evidence_strength": "explicit", "category": "places"}
        ]);
        let brain = router_entite(c);
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entities"), 1);
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entity_creation_proposals"), 0);
    }

    #[test]
    fn un_fait_assez_durable_cree_encore_directement() {
        // Persistance 4 : au-dessus de MIN_ENTITY_PERSISTENCE. C'est une
        // preuve, donc aucune question n'est posée.
        let brain = router_entite(capture_fete(4));
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entities"), 1);
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entity_creation_proposals"), 0);
    }

    #[test]
    fn les_faits_dune_entite_proposee_nentrent_pas_dans_les_files() {
        // Sinon la même chose serait demandée deux fois : une fois « créer
        // Pierre ? », une fois « ce fait sur Pierre ? ».
        let brain = router_entite(capture_fete(1));
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM pending_facts"), 0);
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM review_queue"), 0);
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM facts"), 0);
    }

    #[test]
    fn accepter_cree_lentite_avec_ses_faits() {
        let brain = router_entite(capture_fete(1));
        let id: String = {
            let conn = brain.storage.lock().unwrap();
            conn.query_row("SELECT id FROM entity_creation_proposals", [], |r| r.get(0))
                .unwrap()
        };
        let out = brain.accept_entity_creation(&id, "2026-07-13").unwrap();
        assert_eq!(out["status"], "accepted");
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entities \
                                   WHERE canonical_name = 'Pierre'"), 1);
        // Le fait revient, et par le même chemin qu'à la capture : persistance
        // 1 et evidence explicit le placent sous 0,85, donc en file de faits.
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM pending_facts"), 1);
        assert_eq!(
            compte(&brain, "SELECT COUNT(*) FROM entity_creation_proposals \
                            WHERE status = 'accepted' AND created_entity_id IS NOT NULL"),
            1
        );
    }

    #[test]
    fn refuser_ne_cree_rien_et_ne_redemande_pas() {
        let brain = router_entite(capture_fete(1));
        let id: String = {
            let conn = brain.storage.lock().unwrap();
            conn.query_row("SELECT id FROM entity_creation_proposals", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(brain.reject_entity_creation(&id).unwrap()["status"], "rejected");
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entities"), 0);
        // Trancher deux fois ne rouvre rien, et se DIT : un second accept qui
        // répondrait « accepted » se lirait comme une réussite chez l'appelant.
        let rejoue = brain.accept_entity_creation(&id, "2026-07-13").unwrap();
        assert_eq!(rejoue["status"], "already_resolved");
        assert_eq!(rejoue["resolution"], "rejected");
        assert_eq!(brain.reject_entity_creation(&id).unwrap()["status"],
                   "already_resolved");
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entities"), 0);
    }

    #[test]
    fn la_meme_question_ne_sempile_pas() {
        let brain = router_entite(capture_fete(1));
        let ctx = RouteContext {
            now: "2026-07-14T12:00:00".into(),
            today: "2026-07-14".into(),
            intentions_cutoff: "2026-07-12T12:00:00".into(),
            now_sql: "2026-07-14 12:00:00".into(),
        };
        {
            let conn = brain.storage.lock().unwrap();
            conn.execute("INSERT INTO inbox (id, content) VALUES ('c2', 'x')", [])
                .unwrap();
        }
        brain
            .route_capture(&json!({"id": "c2", "content": "x"}), &capture_fete(1), &ctx)
            .unwrap();
        assert_eq!(compte(&brain, "SELECT COUNT(*) FROM entity_creation_proposals"), 1);
    }

    // route_capture reads `classified["language"]` and persists it on the note.
    // A note-only capture needs neither the embedder nor the network.
    #[test]
    fn route_capture_persists_note_language() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("s.db");
        let brain = Brain::open(db.to_str().unwrap(), None).unwrap();
        {
            let conn = brain.storage.lock().unwrap();
            conn.execute(
                "INSERT INTO inbox (id, content) VALUES ('c1', ?1)",
                params!["Je me demande si je devrais arrêter le café"],
            )
            .unwrap();
        }
        let entry = json!({"id": "c1", "content": "Je me demande si je devrais arrêter le café"});
        let classified = json!({
            "language": "fr",
            "atomic_note": "Je me demande si je devrais arrêter le café",
            "atomic_note_kind": "note",
            "is_ephemeral": false,
            "summary": "réflexion sur le café",
            "entities": [],
            "relations": [],
            "project_entries": [],
            "classification_confidence": 1.0
        });
        let ctx = RouteContext {
            now: "2026-07-13T12:00:00".into(),
            today: "2026-07-13".into(),
            intentions_cutoff: "2026-07-11T12:00:00".into(),
            now_sql: "2026-07-13 12:00:00".into(),
        };
        brain.route_capture(&entry, &classified, &ctx).unwrap();
        let conn = brain.storage.lock().unwrap();
        let lang: Option<String> = conn
            .query_row(
                "SELECT language FROM atomic_notes WHERE provenance_capture_id = 'c1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(lang.as_deref(), Some("fr"));
    }

    /// SYN-182 — helper for the three gaps below. Routes one capture and hands
    /// back the columns the ticket is about, so each test states its own case
    /// instead of repeating the same twelve lines of scaffolding.
    #[allow(clippy::type_complexity)]
    fn route_one(classified: Value) -> (Option<String>, Option<String>, String,
                                        Option<String>, i64) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("s.db");
        let brain = Brain::open(db.to_str().unwrap(), None).unwrap();
        {
            let conn = brain.storage.lock().unwrap();
            conn.execute("INSERT INTO inbox (id, content) VALUES ('c1', 'x')", [])
                .unwrap();
        }
        let entry = json!({"id": "c1", "content": "x"});
        let ctx = RouteContext {
            now: "2026-08-21T12:00:00".into(),
            today: "2026-08-21".into(),
            intentions_cutoff: "2026-08-19T12:00:00".into(),
            now_sql: "2026-08-21 12:00:00".into(),
        };
        brain.route_capture(&entry, &classified, &ctx).unwrap();
        let conn = brain.storage.lock().unwrap();
        conn.query_row(
            "SELECT owner, review_reason, review_status, event_date, event_recurring \
             FROM atomic_notes WHERE provenance_capture_id = 'c1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
    }

    // ── SYN-188 — renommage déclaré en capture ──────────────────────────

    /// Route une capture qui déclare un renommage, sur une base où l'entité
    /// existe déjà ou non. Rend (nom canonique après coup, propositions).
    fn renommer(entite_existe: bool, canonical: &str, renamed_to: Value)
        -> (String, Vec<(String, String)>) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("s.db");
        let brain = Brain::open(db.to_str().unwrap(), None).unwrap();
        {
            let conn = brain.storage.lock().unwrap();
            conn.execute("INSERT INTO inbox (id, content) VALUES ('c1','x')", []).unwrap();
            if entite_existe {
                conn.execute(
                    "INSERT INTO entities (id, type, canonical_name, aliases, mention_count) \
                     VALUES ('e1','project',?1,'[]',3)",
                    params![canonical],
                ).unwrap();
            }
        }
        let mut classified = base_note("note");
        classified["entities"] = json!([{
            "canonical_name": canonical, "type": "project", "renamed_to": renamed_to,
            "facts": [{"predicate": "is_in_phase", "value": "test",
                       "persistence_value": 4, "evidence_strength": "explicit"}]
        }]);
        let ctx = RouteContext {
            now: "2026-08-25T12:00:00".into(),
            today: "2026-08-25".into(),
            intentions_cutoff: "2026-08-23T12:00:00".into(),
            now_sql: "2026-08-25 12:00:00".into(),
        };
        brain
            .route_capture(&json!({"id": "c1", "content": "x"}), &classified, &ctx)
            .unwrap();
        let conn = brain.storage.lock().unwrap();
        let nom: String = conn
            .query_row("SELECT canonical_name FROM entities LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let props = {
            let mut stmt = conn
                .prepare("SELECT current_name, proposed_name FROM entity_rename_proposals")
                .unwrap();
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        (nom, props)
    }

    #[test]
    fn a_declared_rename_proposes_and_never_applies() {
        // Le nom canonique titre la fiche, sort dans le digest et remonte en
        // recherche : c'est le nom que l'utilisateur LIT comme étant sa mémoire.
        // Un modèle ne le change pas.
        let (nom, props) = renommer(true, "Synapse", json!("Sinam"));
        assert_eq!(nom, "Synapse", "le nom canonique ne doit PAS avoir bougé");
        assert_eq!(props, vec![("Synapse".to_string(), "Sinam".to_string())]);
    }

    #[test]
    fn a_rename_toward_the_same_name_proposes_nothing() {
        // Une variante de casse n'est pas un renommage. Sans ce garde-fou, la
        // file se remplirait de questions qui ne changent rien.
        let (_, props) = renommer(true, "Synapse", json!("synapse"));
        assert!(props.is_empty());
    }

    #[test]
    fn a_brand_new_entity_is_never_renamed() {
        // Elle porte déjà le nom qu'on vient de lui donner : il n'y a rien à
        // renommer, et proposer reviendrait à demander d'arbitrer une capture
        // contre elle-même.
        let (_, props) = renommer(false, "Atlas", json!("Atlas v2"));
        assert!(props.is_empty());
    }

    // ── SYN-189 — négation d'un fait ────────────────────────────────────

    /// Sème une entité et ses faits vivants, puis route une capture qui ne fait
    /// QUE nier. `entities: []` est volontaire : c'est la forme d'une capture
    /// dont toute la charge est la négation, et celle où la passe doit tourner
    /// alors même que `step4_route` ne s'exécute pas.
    /// Un fait tel que le test le relit : prédicat, péremption, successeur.
    type FactRow = (String, Option<String>, Option<String>);

    fn negate(seed: &[(&str, &str, &str)], obsoleted: Value) -> (Vec<FactRow>, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("s.db");
        let brain = Brain::open(db.to_str().unwrap(), None).unwrap();
        {
            let conn = brain.storage.lock().unwrap();
            conn.execute("INSERT INTO inbox (id, content) VALUES ('c1','x')", []).unwrap();
            conn.execute(
                "INSERT INTO entities (id, type, canonical_name) VALUES ('e1','person','Pierre')",
                [],
            ).unwrap();
            for (i, (pred, val, prov)) in seed.iter().enumerate() {
                conn.execute(
                    "INSERT INTO facts \
                     (id, entity_id, predicate, value, confidence, provenance_capture_id) \
                     VALUES (?1,'e1',?2,?3,1.0,?4)",
                    params![format!("f{i}"), pred, val, prov],
                ).unwrap();
            }
        }
        let mut classified = base_note("note");
        classified["obsoleted_facts"] = obsoleted;
        let ctx = RouteContext {
            now: "2026-08-25T12:00:00".into(),
            today: "2026-08-25".into(),
            intentions_cutoff: "2026-08-23T12:00:00".into(),
            now_sql: "2026-08-25 12:00:00".into(),
        };
        brain
            .route_capture(&json!({"id": "c1", "content": "x"}), &classified, &ctx)
            .unwrap();
        let conn = brain.storage.lock().unwrap();
        let facts = {
            let mut stmt = conn
                .prepare("SELECT predicate, obsoleted_at, obsoleted_by FROM facts ORDER BY id")
                .unwrap();
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        let proposals = {
            let mut stmt = conn
                .prepare("SELECT reason FROM fact_negation_proposals ORDER BY rowid")
                .unwrap();
            let rows = stmt
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<Vec<String>, _>>()
                .unwrap();
            rows
        };
        (facts, proposals)
    }

    #[test]
    fn a_certain_negation_retires_the_fact_without_a_successor() {
        // `obsoleted_by` doit rester NULL : rien n'a remplacé ce fait, il a
        // cessé. C'est ce qui distingue une négation d'un supersede SYN-37, et
        // c'est ce que lit `/fact/{id}/restore` pour le rappeler.
        let (facts, proposals) = negate(
            &[("works_at", "Acme", "c0")],
            json!([{"entity_canonical": "Pierre", "predicate": "works_at", "value": "Acme"}]),
        );
        assert!(facts[0].1.is_some(), "le fait devait être périmé");
        assert!(facts[0].2.is_none(), "aucun successeur ne doit être inscrit");
        assert!(proposals.is_empty());
    }

    #[test]
    fn a_negation_reaches_the_whole_single_valued_family() {
        // Nier `works_at` doit atteindre `employer` : c'est la même affirmation
        // sous deux noms, et c'est exactement la portée qu'aurait eue une
        // nouvelle valeur.
        let (facts, proposals) = negate(
            &[("employer", "Acme", "c0")],
            json!([{"entity_canonical": "Pierre", "predicate": "works_at", "value": "Acme"}]),
        );
        assert!(facts[0].1.is_some());
        assert!(proposals.is_empty());
    }

    #[test]
    fn a_value_the_memory_does_not_hold_is_queued_never_applied() {
        // La capture nie Acme, la mémoire dit Globex. Les deux se contredisent
        // sur CE QUI était vrai ; périmer Globex trancherait une contradiction
        // que personne n'a regardée.
        let (facts, proposals) = negate(
            &[("works_at", "Globex", "c0")],
            json!([{"entity_canonical": "Pierre", "predicate": "works_at", "value": "Acme"}]),
        );
        assert!(facts[0].1.is_none(), "le fait en mémoire ne doit pas bouger");
        assert_eq!(proposals, vec!["valeur_differente".to_string()]);
    }

    #[test]
    fn a_claim_denied_without_a_value_retires_all_of_its_values() {
        // « il n'a plus de téléphone » ne nomme aucune valeur parce qu'il les
        // nie toutes. Ce n'est pas une ambiguïté, c'est la portée de l'énoncé.
        let (facts, proposals) = negate(
            &[("phone", "06", "c0"), ("phone_number", "07", "c0")],
            json!([{"entity_canonical": "Pierre", "predicate": "phone", "value": null}]),
        );
        assert!(facts[0].1.is_some() && facts[1].1.is_some());
        assert!(proposals.is_empty());
    }

    #[test]
    fn an_approximate_predicate_is_shown_never_acted_on() {
        // Hors famille, `worked_at` et `works_at` ne se rejoignent que par la
        // signature de SYN-190 — assez proche pour être montré, jamais assez
        // pour agir. C'est le résidu que SYN-190 laisse, et la raison pour
        // laquelle il bloquait ce ticket.
        let (facts, proposals) = negate(
            &[("supported_tagging", "manual", "c0")],
            json!([{"entity_canonical": "Pierre", "predicate": "supports_tagging",
                    "value": "manual"}]),
        );
        assert!(facts[0].1.is_none());
        assert_eq!(proposals, vec!["approximatif".to_string()]);
    }

    #[test]
    fn a_capture_never_negates_its_own_writes() {
        // Le même texte peut poser une nouvelle valeur et retirer l'ancienne.
        // Sans ce garde-fou, il retirerait celle qu'il vient d'écrire.
        let (facts, proposals) = negate(
            &[("works_at", "Acme", "c1")],
            json!([{"entity_canonical": "Pierre", "predicate": "works_at", "value": "Acme"}]),
        );
        assert!(facts[0].1.is_none());
        assert!(proposals.is_empty(), "et rien à arbitrer non plus");
    }

    #[test]
    fn denying_something_about_an_unknown_entity_writes_nothing() {
        // NEG-c : une négation ne crée jamais de nœud, et jamais de « fait
        // négatif ». Silence est la bonne réponse.
        let (facts, proposals) = negate(
            &[("works_at", "Acme", "c0")],
            json!([{"entity_canonical": "Marie", "predicate": "has_pet", "value": "chat"}]),
        );
        assert!(facts[0].1.is_none());
        assert!(proposals.is_empty());
    }

    fn base_note(kind: &str) -> Value {
        json!({
            "language": "fr",
            "atomic_note": "…",
            "atomic_note_kind": kind,
            "is_ephemeral": false,
            "summary": "",
            "entities": [],
            "relations": [],
            "project_entries": [],
            "classification_confidence": 1.0
        })
    }

    // SYN-182 · A — "Marie told me she had to call the dentist". The prompt has
    // promised "never as the author's own" since SYN-85 with nothing behind it:
    // there was no column to hold the answer, so the task landed in the author's
    // backlog anyway. A named owner must survive to the row; anything else and
    // the digest filter has nothing to filter on.
    #[test]
    fn reported_speech_keeps_the_owner_off_the_author() {
        let mut c = base_note("task");
        c["atomic_note_owner"] = json!("Marie");
        let (owner, _, status, _, _) = route_one(c);
        assert_eq!(owner.as_deref(), Some("Marie"));
        assert_eq!(status, "confirmed", "un propriétaire n'est pas un doute");

        // Le cas normal reste NULL — c'est ce que lisent toutes les lignes
        // écrites avant l'existence de la colonne, et « mes tâches » compte
        // dessus.
        let (owner, ..) = route_one(base_note("task"));
        assert_eq!(owner, None);
    }

    // SYN-182 · B — the queue was built for task/event only, so an episode the
    // model was 20% sure about was still written `confirmed`. A doubtful note
    // clutters; a doubtful episode ASSERTS that something took place.
    #[test]
    fn a_doubtful_episode_now_reaches_the_validation_queue() {
        for (kind, attendu) in [
            ("episode", "existence_douteuse"),
            ("note", "existence_douteuse"),
            ("task", "perte_possible"),
            ("event", "perte_possible"),
        ] {
            let mut c = base_note(kind);
            c["classification_confidence"] = json!(0.2);
            let (_, reason, status, ..) = route_one(c);
            assert_eq!(status, "pending", "{kind} hésitant doit aller en file");
            assert_eq!(reason.as_deref(), Some(attendu), "motif pour {kind}");
        }
    }

    // SYN-182 · C — two distinct doubts, and the costlier one is the recurrence:
    // it commits us to notifying the user every year, forever. The prompt only
    // justifies recurrence for a birthday, which is an `event`, so recurrence on
    // any other kind was decided without a rule.
    #[test]
    fn an_inferred_recurrence_is_validated_a_confident_birthday_is_not() {
        let mut c = base_note("episode");
        c["event_date"] = json!("2026-04-18");
        c["event_recurring"] = json!(true);
        let (_, reason, status, date, recurring) = route_one(c);
        assert_eq!(status, "pending");
        assert_eq!(reason.as_deref(), Some("recurrence_inferee"));
        // …et surtout la date SURVIT. Avant SYN-182, `durable` excluait
        // l'épisode : « notre rencontre avec Marie était le 18 avril » était
        // écrit avec NULL et 0, donc l'anniversaire de rencontre était détruit
        // à l'insertion, sans qu'aucune règle de récurrence puisse le sauver.
        assert_eq!(date.as_deref(), Some("2026-04-18"));
        assert_eq!(recurring, 1);

        // L'anniversaire nommé et assumé reste hors file : c'est le seul cas que
        // le prompt couvre vraiment.
        let mut c = base_note("event");
        c["event_date"] = json!("2026-06-16");
        c["event_recurring"] = json!(true);
        let (_, reason, status, ..) = route_one(c);
        assert_eq!(status, "confirmed");
        assert_eq!(reason, None, "pas de motif sans doute");
    }
}
