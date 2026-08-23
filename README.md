# sinam-core

The single compiled brain of [sinam](https://github.com/alexisraitano-myffu/sinam-backend): embeddings, storage, routing, decay, summaries, LLM orchestration and P2P sync, written once in Rust and consumed everywhere.

- **Desktop host** (macOS/Windows FastAPI backend): via the PyO3 binding (`crates/sinam-core-py`).
- **Mobile apps** (Android/iOS): via the UniFFI binding (`crates/sinam-core-ffi`).

One implementation, zero logic divergence between platforms.

## What's in the core

Everything the "Dream Cycle" does lives here as Rust modules in `crates/sinam-core/src/`:

| Module | Responsibility |
| -- | -- |
| `embedder.rs` | Local ONNX embeddings (fastembed, `paraphrase-multilingual-MiniLM-L12-v2`, 384-d, L2-normalized). Long text is **chunked** into ~128-token windows, one vector per window, best chunk wins (SYN-118) |
| `storage.rs` · `schema.rs` · `sql.rs` · `migrate.rs` | SQLite substrate (`rusqlite` + `sqlite-vec`), schema, the SQL gateway the host writes through, migrations |
| `routing.rs` | The pipeline: classify → resolve/coreference → confidence-score → route (fact / note / relation / ephemeral) → fact⇄relation dedup → `review_status` gating |
| `llm.rs` | Prompt assembly, blocking HTTP (`ureq`, rustls), JSON parsing, and the **two-call classifier** (see below). Provider-agnostic since SYN-150: `Anthropic`, `OpenAiCompatible`, or `Local` (an on-device backend passed in as a foreign callback, SYN-155). Every provider is normalized back to one Anthropic-shaped response, so the rest of the core never knows which one ran |
| `usage.rs` | What each LLM call actually consumed (SYN-160): one row per call, never aggregated at write time, the four token buckets kept apart because they are priced differently. Rows replicate like any other table |
| `decay.rs` | Graceful forgetting: `memory_strength = exp(-Δdays/τ)`. An `episode` fades faster than a note, and is never deleted |
| `summaries.rs` · `digest.rs` | Derived entity summaries (regenerated from active facts/relations) and the weekly digest |
| `resources.rs` | URL fetch + summarize into searchable resources |
| `sync.rs` | The P2P sync engine (see below) |
| `snapshot.rs` | A local read snapshot for app replicas: the same JSON the host's read endpoints return, computed offline (space, devices, project facts, the "to validate" queues, the living-map graph) |
| `actions.rs` | The local write rail: `apply_action` mirrors the host's write endpoints, so an app can act offline and replay. A stale replay returns `not_found`/`skipped`, never a blocking error |
| `pairing.rs` | Authenticated ECDH device pairing, plus a 6-digit code channel (SPAKE2 + confirmation MAC) for camera-less joiners |

## Prompts are data

Prompts are **not compiled in**. They live as versioned files under [`prompts/`](prompts/), listed in a [`manifest.json`](prompts/manifest.json) that carries a single integer version for the set (currently **17**), and are read at runtime. They can be edited without recompiling and are synchronized between devices as ordinary data, so every platform runs identical prompts.

**A prompt is data that has to be copied onto every surface.** Changing a file here ships it *nowhere*: each host reads its own deployed copy (`~/.synapse/prompts/`, override `SYNAPSE_PROMPTS_DIR`), the mobile apps carry theirs as bundled assets, and the desktop installer carries a third inside its bundled backend. Deploy the prompts before, or with, any build that reads them.

### The classifier is two calls (SYN-171)

One capture, two independent requests:

| Prompt | Decides |
| -- | -- |
| `classifier-note.md` | Routing and prose: `atomic_note`, `atomic_note_kind` (`note` / `task` / `event` / `episode`), `atomic_note_owner`, `event_date`, `event_recurring`, `is_ephemeral`, `classification_confidence` |
| `classifier-graph.md` | The graph: `entities`, their `facts`, `relations`, `project_entries` |

`llm::merge_halves` merges them **in the core**, never in a host: each half owns its own keys, so a merge rule duplicated on two hosts would drift and lose fields without ever raising. `classifier.md` is kept as the superseded single-call fallback and as a size reference; the core no longer reads it.

The split makes the "a note is never absorbed by the facts" invariant **structural** rather than repeated four times in one prompt: the graph half has no `atomic_note` field, so it cannot set it to null. Measured on the 61-case harness, Haiku, t=0: routing identical 61/61 against the validated single-call baseline.

Two consequences worth knowing before resizing a half:

- Freed from competing with the note, the graph half **over-extracts** unless something stops it (19 facts became 43, including "bread" and "true"). Hence the sobriety rule at the top of `classifier-graph.md`: the freedom it is granted is about *suppression*, not volume, and `"facts": []` is the correct default answer.
- **Haiku caches a system prefix only above 4096 tokens.** The single call (~5000) was cached; the two halves (~2900 and ~2400) are not. It is a cliff, not a slope: growing a half past the threshold costs *less* than leaving it just under.

### Multilingual by construction (SYN-119)

The prompts are **EN-base**: their skeleton is English, and the output follows the capture's language. What the model *writes* (note, summaries, project content) is in the capture's language and is never translated; what the graph is *made of* (`atomic_note_kind`, entity `type`, fact and relation `predicate`, `category`) stays English snake_case, as an interlingua. The classifier emits an ISO 639-1 `language` field, so detection costs no extra call and no extra dependency. Adding a language is zero core work.

## P2P sync (SYN-112)

A **homemade** sync engine, not a third-party CRDT. cr-sqlite was dormant and rejected the schema, and Automerge is the wrong model for ~20 relational tables — and because an owner-lock means a single device runs the Dream Cycle, all derived tables are effectively single-writer, so the engine stays deliberately small: a `sync_log` change journal (a version map, not an event log), a hybrid logical clock computed in pure SQL, per-column **last-writer-wins** merge, and tombstones, over a versioned protocol. Any writer — the core, the Python host through the `sql.rs` gateway, even a `sqlite3` CLI — journals correctly with zero registration.

## Crates

| Crate | Role |
| -- | -- |
| `crates/sinam-core` | Pure Rust library (the brain) |
| `crates/sinam-core-py` | PyO3 binding, built as a Python wheel with maturin |
| `crates/sinam-core-ffi` | UniFFI binding for Kotlin and Swift |

## Model files are data

The embedding model (ONNX + tokenizer files) is never compiled in. Hosts pass a directory containing the model files at runtime; apps bundle them as assets. This keeps vectors byte-compatible across platforms and satisfies App Store rule 2.5.2 (no downloaded code).

## Build

```bash
cargo build                        # desktop (onnxruntime downloaded at build time)
cargo test                         # SYNAPSE_MODEL_DIR=<model dir> to run embedding tests

# Python wheel (desktop host)
cd crates/sinam-core-py && maturin build --release

# Android (the app ships libonnxruntime.so and ort loads it dynamically)
cargo ndk -t arm64-v8a -t x86_64 build -p sinam-core-ffi --no-default-features --features ort-dynamic --release
```

## License

Apache-2.0
