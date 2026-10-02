# Research Note — HEX (Anomaly) and opportunities for Lyn

**Date:** 2026-09-29  
**Scope:** Public `anomalyco/hex` repository and Lyn's accepted requirements / speech decision. This is a research note, not a claim that HEX has been run on Lyn's reference machine.

## Executive recommendation

HEX is a useful reference for turning local speech into a fast, dependable interaction. The strongest ideas for Lyn are its native inference path, warm model/session ownership, explicit model and language selection, measured startup/inference/tail latency, and recovery-oriented UX. Its headline performance numbers do not transfer to Lyn: the published measurements are mainly Apple Silicon with Metal, and the tested English Parakeet configurations do not establish French quality or Linux CPU performance.

Keep transcription optional and keep durable voice capture independent of it. Before changing Lyn's pinned Whisper `base` package, run a small controlled Linux x86_64 evaluation against the actual French and English recordings Lyn users make. Include at least one multilingual candidate such as Parakeet TDT v3 only if its Linux CPU runtime, model distribution/license, RAM, and French accuracy can be validated. Do not select a model from speed claims alone.

## What HEX is today

The repository describes a Rust-native local dictation app, primarily for Apple Silicon/macOS, with a Linux beta. The current README says it offers model and language choices; its current architecture includes Parakeet through `transcribe.cpp` and a Linux transcription adapter. HEX's own September reconnaissance names Parakeet Unified English Q8_0 and Cohere Q8_0 for its published exploratory Linux/CPU-style benchmark excerpt, while its model-switching documentation distinguishes English with Parakeet and French with Whisper. Model availability therefore depends on platform and selected model; “HEX uses Parakeet” is not a sufficient description.

The app is designed for dictation-and-paste, not saved voice notes. Its lifecycle, audio handling, and latency targets differ from Lyn's post-save caption enrichment. HEX's README also labels Linux a beta with a smaller feature set; macOS/Metal results should not be treated as Linux results.

## Transferable ideas

| HEX practice | Value for Lyn | Adaptation |
|---|---|---|
| Native Rust inference integration and a dedicated transcription/session owner | Fewer process-start and integration boundaries; makes warm state and cancellation tractable | Consider a replaceable native adapter after benchmarking. Preserve Rust ownership and typed errors. Avoid a long-lived process unless measured improvement justifies its memory/lifecycle cost. |
| Separate capture from inference; bounded queues and explicit cancellation | Keeps the user's primary action responsive under model load or failure | Lyn already saves first. Keep that invariant; expose cancel/skip/retry for enrichment where useful, and ensure shutdown/cancellation cannot affect saved audio. |
| Explicit model and language choices, with prepared/available states | Makes language/quality tradeoffs visible and avoids silently using an unsuitable model | Offer a small curated catalog only after each artifact/runtime is verified for Lyn's supported platform. A remembered language can reduce wrong-language output. Keep the current simple option until there is a validated alternative. |
| Warm model/session reuse and prewarming before activation | May avoid repeatedly paying model load cost for each short note | Measure current per-note process/model load separately from inference. If load dominates, test a single Rust-owned warm worker; model switching should preserve a previous working model on failure. |
| Benchmarks include load time, median and p95, peak RSS, and transcript quality | Prevents optimizing only average speed or accepting worse output | Adopt a reproducible, private corpus and report cold/warm latency, RTF, p50/p95, CPU/RSS, and WER/CER together. Include French (including local accents), English, technical terms, short clips, noise, hesitations, and silence. |
| Bounded jobs, explicit lifecycle states, actionable model preparation failures | Users can tell whether a model is missing, loading, ready, or failed | Keep Settings state truthful and recovery explicit. Do not present a model as installed/ready until integrity checks and a real bounded startup probe succeed. |
| Correction vocabulary / replacements | Can repair recurring names and project-specific terms without replacing the recognizer | Consider user-controlled local replacements for generated captions only if they remain clearly editable and never alter the saved audio or user-authored caption. This is secondary to model quality. |

HEX also shows a privacy tradeoff worth avoiding: its README discloses that diagnostic logs can include transcript text and app/URL metadata. Lyn's existing rule against logging captures/transcripts is the better fit and should remain.

## Speech model assessment for Lyn

Lyn currently pins whisper.cpp v1.9.2 and multilingual Whisper `base` (`ggml-base.bin`), CPU-only, with a ~150 MB download and an upstream expected memory estimate around 388 MB. It is opt-in and post-save. The accepted decision explicitly says the memory figure is not a hard peak limit and calls for reference-machine latency/RSS measurement before release. That is the relevant evidence gap: the model is called lightweight, but Lyn's docs do not establish measured French transcription quality or actual reference-machine speed/RSS. In the current adapter, each job starts a new `whisper-cli` process with up to four threads and automatic language detection; therefore model/process startup is paid per job unless the runtime itself caches it externally. Separate startup from inference before deciding that model size alone causes the poor experience.

HEX's current public README describes switching between Parakeet for English and Whisper for French in its UI. This is a useful signal that one model need not be optimal for every language. Its September 4 benchmark found short English clips around 79–115 ms median for Parakeet and 92–127 ms for Cohere on an M2 Max, but also recorded a Parakeet 7.3-second outlier and stated the corpus was synthetic English and the experiment did not change production defaults. The Parakeet path uses a Metal encoder with CPU decoding. Those numbers do not compare against Lyn's Whisper base, do not measure French, and do not represent Lyn's Linux CPU target.

**Recommendation:** do not adopt HEX's Parakeet model or engine wholesale. First determine what “really bad” means in Lyn by capturing a local, consented evaluation corpus with ground-truth transcripts and separating:

1. microphone/WAV quality and clipping/silence;
2. model load and process startup;
3. inference time and resource use;
4. recognition errors by language/accent/noise/domain;
5. caption truncation or post-processing effects.

Then compare the current Whisper base with one or two supported multilingual candidates under the same Linux CPU runtime and identical 16 kHz mono PCM. Candidate inclusion is conditional on a compatible and maintainable runtime, a pinned source and digest, redistribution/license review, and a reproducible clean-install path. A larger Whisper model might improve quality but may cost substantially more RAM/latency; quantized or alternate engines can shift the tradeoff, so measure rather than infer.

Use word error rate (WER) and character error rate (CER) by language, with manually verified references. Report numbers separately for French and English, and inspect names/numbers/technical vocabulary; aggregate scores can hide poor French or code-term recognition. Run cold and warm passes, at idle and under ordinary desktop load. Compare p50 and p95 wall time, real-time factor, peak RSS, model bytes, and CPU utilization. Keep the corpus private and excluded from Git; do not retain real user audio by default.

## Suggested Lyn sequence

1. **Instrument a baseline:** record safe aggregate timings (queue wait, process/model startup, inference, total enrichment) and peak memory without transcript/audio logging.
2. **Build the local corpus:** at least 12 short representative clips per language, with consent and human references; cover short phrases, normal sentences, technical words, numbers, accents, pauses, and ordinary background noise.
3. **Compare candidates on the target:** same host, audio, runtime settings, repeated randomized runs; include model load, p50/p95, RTF, RSS, WER/CER, and failure rate.
4. **Improve the experience:** clearly label transcript as generated, show pending/failed/retry states without blocking saved-note access, permit language selection if it materially improves accuracy, and preserve manual-caption precedence.
5. **Only then change distribution:** if an alternate candidate wins on quality at acceptable resource cost, add it as an explicitly identified, immutable artifact with checksum, attribution, license, extraction rules, and regression fixtures. Keep the existing model available until migration and owner acceptance are complete.

## Evidence limits

- HEX's repository is actively evolving; this snapshot was reviewed on 2026-09-29. Its README and docs distinguish shipped behavior, plans, and experiments; this note treats performance reconnaissance as exploratory.
- The cited HEX performance report is an M2 Max experiment and does not prove behavior on Lyn's Pop!_OS / Ubuntu-compatible x86_64 target.
- The model benchmark inputs are synthetic/English for the September reconnaissance. They do not establish French accuracy, African French accent coverage, or dictation quality.
- No HEX binary was installed or benchmarked in this workspace. No model change is implemented by this note.

## Sources

- [Anomaly HEX repository and current README](https://github.com/anomalyco/hex)
- [HEX transcription benchmark and its historical caveats](https://github.com/anomalyco/hex/blob/main/docs/research/transcription-benchmark.md)
- [HEX September 2026 inference performance reconnaissance](https://github.com/anomalyco/hex/blob/main/docs/research/performance-2026-09-04.md)
- [HEX engineering guide: native transcription/session architecture](https://github.com/anomalyco/hex/blob/main/AGENTS.md)
- [Lyn local speech distribution decision](09_local_speech_distribution_decision.md)
- [Lyn architecture and provisional latency budgets](05_architecture.md)
