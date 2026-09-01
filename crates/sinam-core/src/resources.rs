//! Resource pipeline (T5 port of `dream_cycle/resources.py`): fetch
//! a URL, extract readable text, summarise it with the LLM, store it
//! (searchable via its embedded summary).
//!
//! HTML extraction is a dependency-free tag stripper (the Python original was
//! stdlib `html.parser`): skip script/style/nav/…, grab `<title>`, decode
//! common entities. The text is rough, but the LLM summarises it well.
//! Fetch failures and per-URL errors are non-fatal, like Python: a capture's
//! routing never blocks on a dead link.
//!
//! Everything runs on the Brain's OWN connection (network + LLM happen before
//! the DB write, no lock held) — hosts call it outside their transactions.

use std::collections::HashSet;
use std::time::Duration;

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::embedder::CoreError;
use crate::llm::{load_prompt, post_messages_text, LlmConfig};
use crate::routing::{new_uuid, Brain};

const SKIP_TAGS: [&str; 8] =
    ["script", "style", "noscript", "head", "nav", "footer", "header", "svg"];
const MAX_CONTENT: usize = 50_000; // cap stored text (chars) — articles can be huge
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// All http(s) URLs in a capture, de-duplicated, order-preserving. Port of
/// `URL_RE` (`https?://[^\s<>"'\)\]]+` + rstrip of trailing punctuation).
pub fn extract_urls(text: &str) -> Vec<String> {
    const STOP: &[char] = &['<', '>', '"', '\'', ')', ']'];
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    let mut last_end = 0usize;
    for (idx, _) in text.match_indices("http") {
        if idx < last_end {
            continue; // inside the previous match, like a regex scan
        }
        let tail = &text[idx..];
        let scheme_len = if tail.starts_with("https://") {
            8
        } else if tail.starts_with("http://") {
            7
        } else {
            continue;
        };
        let body = &tail[scheme_len..];
        let end = body
            .find(|c: char| c.is_whitespace() || STOP.contains(&c))
            .unwrap_or(body.len());
        if end == 0 {
            continue;
        }
        last_end = idx + scheme_len + end;
        let url = tail[..scheme_len + end].trim_end_matches(['.', ',', ';', ')']);
        if !url.is_empty() && seen.insert(url.to_string()) {
            out.push(url.to_string());
        }
    }
    out
}

pub struct PageText {
    pub title: String,
    pub text: String,
}

/// Decode the common HTML entities (named subset + numeric) — the Python
/// parser ran with `convert_charrefs=True`.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let semi = rest
            .as_bytes()
            .iter()
            .take(32)
            .position(|&b| b == b';');
        if let Some(semi) = semi {
            let ent = &rest[1..semi];
            let decoded = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                }
                _ if ent.starts_with('#') => {
                    ent[1..].parse::<u32>().ok().and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(c) = decoded {
                out.push(c);
                rest = &rest[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

/// Port of `_TextExtractor`: title + visible text, skip-tag subtrees dropped.
/// `<title>` wins over the skip counter (it lives inside `<head>`, which is a
/// skip tag). script/style bodies are raw text until their explicit end tag.
pub fn extract_page(html: &str) -> PageText {
    let mut title = String::new();
    let mut chunks: Vec<String> = Vec::new();
    let mut in_title = false;
    let mut skip = 0usize;

    let bytes = html.as_bytes();
    let mut i = 0usize;
    let mut data_start = 0usize;

    fn flush(
        html: &str,
        from: usize,
        to: usize,
        in_title: bool,
        skip: usize,
        title: &mut String,
        chunks: &mut Vec<String>,
    ) {
        if from >= to {
            return;
        }
        let decoded = decode_entities(&html[from..to]);
        if in_title {
            title.push_str(&decoded);
        } else if skip == 0 {
            let t = decoded.trim();
            if !t.is_empty() {
                chunks.push(t.to_string());
            }
        }
    }

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        flush(html, data_start, i, in_title, skip, &mut title, &mut chunks);

        if html[i..].starts_with("<!--") {
            i = html[i..].find("-->").map(|p| i + p + 3).unwrap_or(bytes.len());
            data_start = i;
            continue;
        }
        // Scan to the closing '>' honoring quoted attribute values.
        let mut j = i + 1;
        let mut quote: Option<u8> = None;
        while j < bytes.len() {
            let c = bytes[j];
            match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                }
                None => {
                    if c == b'"' || c == b'\'' {
                        quote = Some(c);
                    } else if c == b'>' {
                        break;
                    }
                }
            }
            j += 1;
        }
        if j >= bytes.len() {
            data_start = bytes.len();
            break; // unterminated tag — drop the tail
        }
        let inner = &html[i + 1..j];
        let closing = inner.starts_with('/');
        let name: String = inner
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        let self_closing = !closing && inner.trim_end().ends_with('/');
        i = j + 1;
        data_start = i;
        if name.is_empty() {
            continue; // <!DOCTYPE …>, processing instructions…
        }
        if closing {
            if SKIP_TAGS.contains(&name.as_str()) {
                skip = skip.saturating_sub(1);
            } else if name == "title" {
                in_title = false;
            }
        } else {
            if SKIP_TAGS.contains(&name.as_str()) && !self_closing {
                skip += 1;
            } else if name == "title" && !self_closing {
                in_title = true;
            }
            // Raw-text elements: `if (a<b)` inside a script must not be
            // parsed as markup — jump straight to the explicit end tag.
            if (name == "script" || name == "style") && !self_closing {
                let close = format!("</{name}");
                match html[i..].to_ascii_lowercase().find(&close) {
                    Some(p) => {
                        i += p;
                        data_start = i;
                    }
                    None => {
                        i = bytes.len();
                        data_start = i;
                    }
                }
            }
        }
    }
    flush(html, data_start, bytes.len(), in_title, skip, &mut title, &mut chunks);

    PageText {
        title,
        text: chunks.join("\n"),
    }
}

/// GET the URL and return {title, text}. None on any network/HTTP/parse
/// failure — the caller treats a fetch miss as non-fatal.
pub fn fetch_and_extract(url: &str, timeout: Duration) -> Option<PageText> {
    let resp = ureq::get(url)
        .timeout(timeout)
        .set("User-Agent", "sinamBot/1.0 (personal memory)")
        .call()
        .ok()?;
    let content_type = resp.header("content-type").unwrap_or("text/html").to_string();
    let body = resp.into_string().ok()?;
    if !content_type.contains("html") {
        // non-HTML (PDF, etc.) — out of scope for V1, store raw text if textual
        let text: String = body.chars().take(MAX_CONTENT).collect();
        if text.is_empty() {
            return None;
        }
        return Some(PageText {
            title: url.to_string(),
            text,
        });
    }
    let page = extract_page(&body);
    let text: String = page.text.chars().take(MAX_CONTENT).collect();
    if text.is_empty() {
        return None;
    }
    let title = page.title.trim().to_string();
    Some(PageText {
        title: if title.is_empty() { url.to_string() } else { title },
        text,
    })
}

/// LLM summary of the extracted text (prompt = data `resource-summary.md`).
/// Falls back to a truncated snippet without a config (offline) or on any
/// LLM/prompt failure — a resource is always storable.
/// Renvoie aussi ce que l'appel a consommé. L'écriture du compteur
/// n'a PAS lieu ici : le contrat de cette fonction est de ne tenir aucun verrou
/// pendant le réseau et le LLM. L'appelant l'enregistre sur la connexion qu'il
/// prend déjà pour l'INSERT.
fn summarize(
    config: Option<&LlmConfig>,
    title: &str,
    text: &str,
) -> (String, crate::usage::LlmUsage) {
    let snippet: String = text.chars().take(300).collect();
    let none = crate::usage::LlmUsage::default();
    let Some(config) = config else {
        return (snippet, none);
    };
    let Ok(system) = load_prompt(&config.prompts_dir, "resource-summary.md") else {
        return (snippet, none);
    };
    let head: String = text.chars().take(8000).collect();
    let params_json = json!({
        "model": config.model,
        // 2-4 phrases demandées + marge de raisonnement, cf. summaries::resummarize.
        "max_tokens": 1024,
        "system": system,
        "messages": [{"role": "user", "content": format!("Title: {title}\n\n{head}")}],
    });
    match post_messages_text(config, &params_json) {
        // Still empty after the retry — fall back to the extracted snippet.
        // The call was billed either way, so its usage travels back regardless.
        Ok((t, used)) if !t.is_empty() => (t, used),
        Ok((_, used)) => (snippet, used),
        Err(_) => (snippet, none),
    }
}

/// Aller chercher la page, ou non.
///
/// La requête apprend au serveur d'en face l'IP, l'heure, l'URL et le
/// user-agent — au moment où l'utilisateur ENREGISTRE, pas au moment où il
/// lit, et pendant le cycle, donc sans qu'il soit devant. Un lien raccourci
/// l'apprend à deux serveurs, le raccourcisseur puis la destination.
///
/// Allumé par défaut : le titre réel et un résumé valent quelque chose. Mais
/// c'est désormais un choix, et il ne l'était pas — sans récupération, il n'y
/// avait pas de ressource du tout.
fn recuperation_autorisee() -> bool {
    !matches!(
        std::env::var("SYNAPSE_FETCH_RESOURCES").as_deref(),
        Ok("0") | Ok("false") | Ok("FALSE")
    )
}

impl Brain {
    /// ENRICHIR un lien avec ce que la page en dit. Réseau + LLM avant l'écriture.
    ///
    /// Ce n'est plus ce qui fait EXISTER une ressource : le routage l'a déjà
    /// enregistrée depuis ce que le classifieur a lu, sans aucune requête. Ici
    /// on ne fait qu'ajouter le titre réel, le texte et un résumé, et échouer
    /// ne coûte donc plus rien — c'est l'ancien couplage qui a rempli la
    /// mémoire d'un mur de connexion et d'une bannière de cookies.
    ///
    /// Le garde n'est plus « cette URL est connue » mais « elle a déjà été
    /// récupérée » : depuis le renversement, la ligne existe toujours avant
    /// d'arriver ici, donc l'ancien garde n'aurait plus jamais laissé passer.
    ///
    /// Rend None si la récupération est coupée ou a échoué.
    pub fn process_resource(
        &self,
        url: &str,
        capture_id: Option<&str>,
        config: Option<&LlmConfig>,
    ) -> Result<Option<String>, CoreError> {
        if !recuperation_autorisee() {
            return Ok(None);
        }
        let ligne = {
            let conn = self.storage.lock()?;
            let deja: Option<(String, Option<String>)> = conn
                .query_row(
                    "SELECT id, fetched_at FROM resources WHERE url = ?1",
                    params![url],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match deja {
                Some((id, Some(_))) => return Ok(Some(id)), // déjà récupérée
                autre => autre.map(|(id, _)| id),
            }
        };

        let Some(page) = fetch_and_extract(url, FETCH_TIMEOUT) else {
            return Ok(None);
        };
        let (summary, used) = summarize(config, &page.title, &page.text);
        // Multi-frame blob: a long summary embeds per window and the
        // resource scorer keeps the best frame.
        let embedding = self.embed_frames(&format!("{}\n{}", page.title, summary));

        let now = crate::decay::resolve_now(None)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let conn = self.storage.lock()?;
        if let Some(config) = config {
            crate::usage::record(&conn, config, crate::usage::Op::Resource, used);
        }
        match ligne {
            // Le cas normal depuis le renversement : la ligne existe, on la
            // complète. Le type n'est PAS touché — il porte la catégorie que le
            // classifieur a donnée, et la page n'a pas voix au chapitre là-dessus.
            Some(id) => {
                conn.execute(
                    "UPDATE resources SET title = ?2, content = ?3, summary = ?4, \
                     embedding = ?5, fetched_at = ?6 WHERE id = ?1",
                    params![id, page.title, page.text, summary, embedding, now],
                )?;
                nommer_la_fiche_du_lien(&conn, &id, url, &page.title, capture_id)?;
                Ok(Some(id))
            }
            // Un lien que le classifieur n'a pas vu passer. Rare, et on ne le
            // perd pas pour autant.
            None => {
                let rid = new_uuid();
                conn.execute(
                    "INSERT INTO resources (id, type, source, url, title, content, summary, \
                     embedding, fetched_at) VALUES (?1,'page',?2,?3,?4,?5,?6,?7,?8)",
                    params![rid, capture_id, url, page.title, page.text, summary, embedding, now],
                )?;
                Ok(Some(rid))
            }
        }
    }

    /// Process every URL found in a capture. Each is independent — one
    /// failure never blocks the others (or the rest of the cycle).
    pub fn process_capture_resources(
        &self,
        content: &str,
        capture_id: Option<&str>,
        config: Option<&LlmConfig>,
    ) -> Result<Vec<String>, CoreError> {
        let mut ids = Vec::new();
        for url in extract_urls(content) {
            if let Ok(Some(rid)) = self.process_resource(&url, capture_id, config) {
                ids.push(rid);
            }
        }
        Ok(ids)
    }
}

/// Donner à la fiche d'un lien le titre de sa page, une fois la page lue.
///
/// Le prompt du graphe exige qu'une ressource appartienne à une entité. Pour un
/// lien nu il n'y a rien à nommer, alors il assume le bouche-trou en toutes
/// lettres : une entité nommée par l'URL, « that is honest, and a later pass may
/// rename it ». Cette passe-là n'existait pas. Le titre réel était déjà en base,
/// dans `resources.title`, à côté d'une fiche qui s'appelait encore `https://…`.
///
/// **Appliquer ou proposer, et la frontière est le nom actuel.** La discipline
/// du routage est qu'un renommage PROPOSE, il n'applique pas : on ne renomme
/// jamais une fiche dans le dos de son propriétaire. Cette règle vaut quand il a
/// choisi le nom. Personne n'a choisi une URL — c'est un bouche-trou que le
/// prompt assume comme tel — donc la remplacer par le titre de la page ne trahit
/// aucune décision, et poser la question remplirait la file de questions sans
/// enjeu. Dès que la fiche porte un vrai nom, on repasse à la proposition.
///
/// Deux gardes, et le premier est le piège de cette fonction : quand la page n'a
/// pas de titre, `fetch_and_extract` renvoie l'URL EN GUISE de titre, à deux
/// endroits. Un renommage naïf remplacerait donc l'URL par l'URL en croyant
/// avoir réussi. Le garde est `titre != url`, jamais `titre.is_empty()`.
///
/// Le TYPE n'est pas touché, comme partout ailleurs dans cette passe : la page
/// nomme, elle ne retype pas.
fn nommer_la_fiche_du_lien(
    conn: &rusqlite::Connection,
    resource_id: &str,
    url: &str,
    titre: &str,
    capture_id: Option<&str>,
) -> Result<(), CoreError> {
    let titre = titre.trim();
    if titre.is_empty() || titre == url {
        return Ok(());
    }
    let lien: Option<(Option<String>,)> = conn
        .query_row(
            "SELECT entity_id FROM resources WHERE id = ?1",
            params![resource_id],
            |r| Ok((r.get(0)?,)),
        )
        .optional()?;
    // Une ressource sans fiche n'a rien à renommer. C'est le cas du lien que le
    // classifieur n'a pas vu passer, celui de la branche voisine.
    let Some((Some(entity_id),)) = lien else {
        return Ok(());
    };
    let fiche: Option<(String, String)> = conn
        .query_row(
            "SELECT canonical_name, aliases FROM entities WHERE id = ?1",
            params![entity_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((nom_actuel, aliases_bruts)) = fiche else {
        return Ok(());
    };
    if nom_actuel.trim().eq_ignore_ascii_case(titre) {
        return Ok(());
    }
    if nom_actuel.trim() != url {
        return crate::routing::record_rename_proposal(
            conn, &entity_id, &nom_actuel, titre, capture_id,
        );
    }
    // L'URL cède la place. L'ancien nom part en alias, exactement comme le fait
    // le renommage manuel : c'est ce qui garde la fiche trouvable par ce que
    // l'utilisateur a pu déjà taper.
    let mut aliases: Vec<String> = serde_json::from_str(&aliases_bruts).unwrap_or_default();
    if !aliases.iter().any(|a| a == &nom_actuel) {
        aliases.push(nom_actuel);
    }
    aliases.retain(|a| !a.eq_ignore_ascii_case(titre));
    conn.execute(
        "UPDATE entities SET canonical_name = ?1, aliases = ?2 WHERE id = ?3",
        params![titre, json!(aliases).to_string(), entity_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn extract_urls_dedups_and_strips_punctuation() {
        let urls = extract_urls(
            "voir https://exemple.fr/article. puis https://exemple.fr/article encore, \
             et http://x.io/y)",
        );
        assert_eq!(
            urls,
            ["https://exemple.fr/article", "http://x.io/y"]
        );
        assert_eq!(extract_urls("rien ici, http:// non plus"), Vec::<String>::new());
    }

    #[test]
    fn extract_page_skips_script_grabs_title() {
        let html = "<html><head><title>Mon Titre</title><style>x{}</style></head>\
                    <body><script>if (a<b) { bad() }</script><p>Bonjour &amp; le monde</p>\
                    <nav>menu</nav><!-- caché --></body></html>";
        let page = extract_page(html);
        assert_eq!(page.title.trim(), "Mon Titre");
        assert!(page.text.contains("Bonjour & le monde"));
        assert!(!page.text.contains("bad()"));
        assert!(!page.text.contains("menu"));
        assert!(!page.text.contains("caché"));
    }

    /// Single-thread HTTP stub good enough for ureq: read the request head,
    /// answer a fixed body, close.
    fn spawn_stub(status: &'static str, content_type: &'static str, body: &'static str) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        port
    }

    #[test]
    fn process_resource_stores_and_is_idempotent() {
        let port = spawn_stub(
            "200 OK",
            "text/html; charset=utf-8",
            "<html><head><title>Article Exemple</title></head>\
             <body><p>Un texte sur les pandas roux.</p></body></html>",
        );
        let dir = tempfile::tempdir().unwrap();
        let brain =
            Brain::open(dir.path().join("r.db").to_str().unwrap(), None).unwrap();
        let url = format!("http://127.0.0.1:{port}/article");
        let rid1 = brain.process_resource(&url, Some("c1"), None).unwrap();
        let rid2 = brain.process_resource(&url, Some("c1"), None).unwrap();
        assert!(rid1.is_some());
        assert_eq!(rid1, rid2, "same URL must not be stored twice");
        let conn = brain.storage.lock().unwrap();
        let (n, title, summary): (i64, String, String) = conn
            .query_row(
                "SELECT COUNT(*), MAX(title), MAX(summary) FROM resources",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(title, "Article Exemple");
        // no LLM config → snippet fallback
        assert!(summary.contains("pandas roux"));
    }

    #[test]
    fn failed_fetch_stores_nothing() {
        let port = spawn_stub("404 Not Found", "text/html", "nope");
        let dir = tempfile::tempdir().unwrap();
        let brain =
            Brain::open(dir.path().join("r.db").to_str().unwrap(), None).unwrap();
        let rid = brain
            .process_resource(&format!("http://127.0.0.1:{port}/x"), None, None)
            .unwrap();
        assert!(rid.is_none());
        let conn = brain.storage.lock().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
    /// Prépare une fiche déjà liée à une ressource, comme le routage l'écrit.
    fn fiche_liee(brain: &Brain, url: &str, nom: &str) -> String {
        let conn = brain.storage.lock().unwrap();
        let eid = new_uuid();
        conn.execute(
            "INSERT INTO entities (id, type, canonical_name) VALUES (?1,'resource',?2)",
            params![eid, nom],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO resources (id, type, source, url, entity_id) \
             VALUES (?1,'page','c1',?2,?3)",
            params![new_uuid(), url, eid],
        )
        .unwrap();
        eid
    }

    fn nom_et_alias(brain: &Brain, eid: &str) -> (String, String) {
        let conn = brain.storage.lock().unwrap();
        conn.query_row(
            "SELECT canonical_name, aliases FROM entities WHERE id = ?1",
            params![eid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    fn propositions(brain: &Brain) -> i64 {
        let conn = brain.storage.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM entity_rename_proposals WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn une_fiche_nommee_par_son_url_prend_le_titre_de_la_page() {
        let port = spawn_stub(
            "200 OK",
            "text/html; charset=utf-8",
            "<html><head><title>Le compost en ville</title></head>\
             <body><p>Un texte.</p></body></html>",
        );
        let dir = tempfile::tempdir().unwrap();
        let brain = Brain::open(dir.path().join("r.db").to_str().unwrap(), None).unwrap();
        let url = format!("http://127.0.0.1:{port}/compost");
        let eid = fiche_liee(&brain, &url, &url);

        brain.process_resource(&url, Some("c1"), None).unwrap();

        let (nom, aliases) = nom_et_alias(&brain, &eid);
        assert_eq!(nom, "Le compost en ville");
        // L'URL reste trouvable : c'est peut-être ce que l'utilisateur a tapé.
        assert!(aliases.contains(&url), "l'ancien nom doit partir en alias : {aliases}");
        assert_eq!(propositions(&brain), 0, "personne n'a choisi l'URL, on n'a rien à demander");
    }

    #[test]
    fn une_fiche_deja_nommee_recoit_une_proposition_et_garde_son_nom() {
        let port = spawn_stub(
            "200 OK",
            "text/html; charset=utf-8",
            "<html><head><title>Figma — UI components</title></head>\
             <body><p>Un texte.</p></body></html>",
        );
        let dir = tempfile::tempdir().unwrap();
        let brain = Brain::open(dir.path().join("r.db").to_str().unwrap(), None).unwrap();
        let url = format!("http://127.0.0.1:{port}/figma");
        let eid = fiche_liee(&brain, &url, "Figma");

        brain.process_resource(&url, Some("c1"), None).unwrap();

        let (nom, _) = nom_et_alias(&brain, &eid);
        assert_eq!(nom, "Figma", "un nom choisi ne se remplace jamais dans le dos");
        assert_eq!(propositions(&brain), 1);
    }

    /// Le piège de cette fonction, pris là où il mord vraiment.
    ///
    /// Sans titre, `fetch_and_extract` renvoie l'URL EN GUISE de titre. Écrit
    /// `titre.is_empty()`, le garde laisse alors passer une proposition de
    /// renommer une fiche qui a un vrai nom... en URL. Le renommage direct,
    /// lui, est déjà couvert par le garde du nom identique — c'est pour ça que
    /// ce test attaque par la fiche NOMMÉE, sinon il ne garde rien : vérifié en
    /// affaiblissant le garde, il reste vert sur une fiche nommée par son URL.
    #[test]
    fn une_page_sans_titre_ne_propose_pas_de_renommer_en_url() {
        let port = spawn_stub(
            "200 OK",
            "text/html; charset=utf-8",
            "<html><body><p>Aucun titre ici.</p></body></html>",
        );
        let dir = tempfile::tempdir().unwrap();
        let brain = Brain::open(dir.path().join("r.db").to_str().unwrap(), None).unwrap();
        let url = format!("http://127.0.0.1:{port}/nu");
        let eid = fiche_liee(&brain, &url, "Le blog de Camille");

        brain.process_resource(&url, Some("c1"), None).unwrap();

        let (nom, _) = nom_et_alias(&brain, &eid);
        assert_eq!(nom, "Le blog de Camille");
        assert_eq!(
            propositions(&brain),
            0,
            "une page sans titre n'a rien à proposer, surtout pas son URL"
        );
    }
}
