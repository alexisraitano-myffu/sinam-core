# Contributing to sinam-core

Thanks for looking. This is a small project with a clear shape, and the fastest
way to get a change merged is to know that shape before you write code.

## What this repo is, and what it is not

`sinam-core` is the **compiled brain**: embeddings, storage, routing, decay,
summaries, LLM orchestration and P2P sync, written once in Rust and consumed by
every host. It is Apache-2.0 and you can run it on its own.

The **applications** are not here. The Android, iOS and desktop clients are a
separate, closed codebase. That is the open-core boundary and it is deliberate:
the engine is open so you can verify what happens to your memory and use it
without us; the apps are what we sell. Feature requests about app UI belong on
[sinam-backend](https://github.com/alexisraitano-myffu/sinam-backend) only if
they concern the HTTP surface, and nowhere public otherwise.

So: bug reports, correctness fixes, performance work, platform support and
anything that makes the engine usable standalone are all welcome. A pull request
that adds product behaviour we have not discussed is likely to sit unmerged, and
that is a waste of your time, not ours.

## Before you write code

**Open an issue first** for anything beyond a typo or an obvious bug fix.
Describe what you observed, what you expected, and on which platform. If you
already know the fix, say so in the issue and we will tell you quickly whether it
is a direction we want. A short exchange up front saves a rewritten branch.

For a bug, the most useful issue contains the smallest input that reproduces it.
This engine is deterministic almost everywhere; if you can hand us a capture and
the wrong output, we can usually see the cause without running anything.

## Building and testing

```bash
cargo build                  # desktop; onnxruntime is downloaded at build time
cargo test                   # the embedding tests need a model directory
```

The embedding tests load a real ONNX model rather than a stub, because a stub
would let a vector regression through. Point them at a local copy:

```bash
SYNAPSE_MODEL_DIR=<path to the model dir> cargo test
```

The model files are **data, not code**: they are not vendored here. The README
says which model and where it comes from.

The other build targets:

```bash
# Python wheel, for the desktop host
cd crates/sinam-core-py && maturin build --release

# Android; the app ships libonnxruntime.so and ort loads it dynamically
cargo ndk -t arm64-v8a -t x86_64 build -p sinam-core-ffi \
  --no-default-features --features ort-dynamic --release
```

iOS is built in CI. You do not need a Mac to contribute to the shared crate:
`cargo test` on Linux exercises everything except the platform bindings.

## The three crates

| crate | what it is |
|---|---|
| `sinam-core` | the engine itself. Almost every change belongs here. |
| `sinam-core-ffi` | the UniFFI binding consumed by Android and iOS. |
| `sinam-core-py` | the PyO3 wheel consumed by the Python host. |

A capability added to the engine has to cross both bindings to be usable
everywhere. If you add one and only wire a single binding, say so in the pull
request rather than leaving it to be discovered later.

## Prompts are production data

`prompts/` holds the classifier and the passes of the nightly cycle, as plain
Markdown. They are **loaded at runtime with no fallback**: a missing or renamed
file is a hard failure at the first classification, not a degraded mode.

Two rules follow, and both have cost us real breakage:

1. **`manifest.json` is the contract.** Adding, removing or renaming a prompt
   means updating the manifest in the same commit. Hosts fetch exactly what the
   manifest declares; a prompt that is not in it does not reach any device.
2. **Changing a prompt changes behaviour for everyone, silently.** A prompt edit
   is a product change, not a copy edit. Explain in the pull request what you
   expect to move, and what you checked. `docs/regles.md` is the master
   description of what the engine is supposed to decide; the prompt derives from
   it, never the other way round.

If your change makes the engine classify differently, the rules document is the
first file to update, and the pull request should say which rule you are
changing.

## Style, and what a good pull request looks like

- Keep the change focused. One reason to merge per pull request.
- Match the surrounding code. This codebase has a voice; follow it rather than
  importing conventions from elsewhere.
- Comments explain **why**, not what. A comment that restates the line above is
  noise; a comment that records the trap someone already fell into is worth more
  than the code it sits on.
- No new dependency without saying in the issue why the standard library or an
  existing dependency will not do. This binary ships inside phone applications.
- Reference issues by their GitHub number. Do not put internal tracker
  identifiers in commits or files here.

## Reporting a vulnerability

**Do not open a public issue for a security problem.** Use GitHub's private
vulnerability reporting on this repository, or write to
`alexis.raitano@myffu.fr`. Describe the class of problem and how to reproduce it;
you do not need to build a working exploit to be taken seriously.

This engine holds people's personal memory and syncs it across a home network.
Anything touching pairing, the sync transport, token handling or the certificate
pinning gets read carefully and fixed quickly.

## Licence

By contributing you agree that your contribution is licensed under the
Apache License 2.0, like the rest of this repository. See [LICENSE](LICENSE).

## Code of conduct

Participation is covered by our [Code of Conduct](CODE_OF_CONDUCT.md). It is
short, and it comes down to treating other people as though you had to sit
across from them.
