# Lyn — G2 Local Speech Distribution Decision

Status: **Parakeet replacement accepted for implementation**, 2026-10-31. Supersedes the 2026-09-01 Whisper package decision. Installed transcription quality, latency and memory acceptance remain pending.

Derived from: [`02_requirements_srs.md`](02_requirements_srs.md), [`05_architecture.md`](05_architecture.md), [`06_api_specification.md`](06_api_specification.md), and [`research_hex_anomaly.md`](research_hex_anomaly.md).

## Decision and user behavior

Lyn offers one optional, CPU-only, local **Parakeet TDT 0.6B v3 Q8** package on Linux x86_64. The owner accepted replacing Whisper because its transcription was unsatisfactory. This is an implementation decision, not evidence that Parakeet's accuracy has passed acceptance.

- Install explicitly through Settings, then enable automatic transcription separately. Both download and enablement remain optional; no model is bundled with core Lyn.
- Save the WAV and close capture immediately. Only committed, captionless voice notes are eligible for background enrichment (FR-056, FR-057, FR-082, INV-02).
- Load on the first transcription, reuse one model for nearby notes, and unload after **60 seconds without a job**. Never prewarm on application startup, recording or installation.
- Run one transcription at a time, with at most four compute threads. Stop the worker when speech is disabled or its package is removed. A timeout, crash or invalid worker response retires that worker; the next explicit job gets a fresh one.
- Recognition remains entirely on-device, without HTTP listeners, cloud fallback, accounts or API fees. Native inference runs in a Lyn-owned child process over private stdin/stdout pipes, isolating native failure and permitting a hard job deadline. An application crash also terminates its child. The spawning enrichment thread remains alive through the warm idle wait (condition-variable wakeups, no database polling); Linux parent-death signaling follows that thread, so it must retire its child before exiting.
- Preserve saved audio on every failure. Generated captions remain editable and revision-checked; a manual caption always wins. The existing caption/search contract is at most 500 characters: longer recognition is shortened for the caption, not retained as a separate full transcript.
- Missing/failed speech remains separate from durable capture. Installation Retry retries installation; transcription failures must be visible and require explicit retry, rather than silently consuming repeated inference attempts.

## Pinned artifacts

Rust owns the fixed artifact allowlist; IPC accepts only `parakeet-tdt-0.6b-v3-q8-v1`, never paths, URLs, digests, runtime flags or model names chosen by the caller.

| Component | Pinned identity | Bytes | SHA-256 |
|---|---|---:|---|
| Engine | `mudler/parakeet.cpp` v0.6.1, `parakeet-v0.6.1-lib-linux-cpu-x64.tar.gz`, C API ABI 10 | 1,490,872 | `5a2b439e83d1b6ab66c55bcfc48751ee48e53727d5b3ea33a6bc0e8626f1397c` |
| Model | `mudler/parakeet-cpp-gguf`, revision `07a08a3a405d20c5434b9a1442f6e6cf571f57ad`, `tdt-0.6b-v3-q8_0.gguf` | 940,663,680 | `4d69a4a6683f4f2d952bad794c1357ca6eb628027695b4699c5a9ad4cd07d757` |

Exact origins:

- `https://github.com/mudler/parakeet.cpp/releases/download/v0.6.1/parakeet-v0.6.1-lib-linux-cpu-x64.tar.gz`
- `https://huggingface.co/mudler/parakeet-cpp-gguf/resolve/07a08a3a405d20c5434b9a1442f6e6cf571f57ad/tdt-0.6b-v3-q8_0.gguf`

Q8 prioritizes recognition quality over the smaller Q4 alternatives. This is about **942 MB total download**, not the earlier roughly 670 MB ONNX estimate: this native GGUF package is a different representation. Peak memory, cold/warm inference time and French/English accuracy are unmeasured. A planning allowance of 1–3 GB RAM for short notes is not a guarantee or enforced cap. The CPU library requires the system C++/OpenMP runtime; a bounded ABI probe refuses incompatible systems during installation. Its flat concurrency API ignores the thread count for one backend. The exact pinned Linux artifact additionally exports `pk::set_num_threads(int)` and `pk::effective_threads()`; the private adapter resolves their exact Itanium symbols, sets the global budget before model loading, and requires the effective value to match `min(4, available_parallelism)`. Missing symbols or a different effective budget refuse; replacing the artifact requires re-auditing this dependency.

## Licenses and provenance

parakeet.cpp is MIT; its exact engine archive includes LICENSE. NVIDIA's Parakeet v3 model is CC BY 4.0, including the converted/quantized derivative. Retain the CC BY 4.0 text and attribution naming NVIDIA, the original model URL, conversion/quantization by mudler/parakeet.cpp, changes made (GGUF Q8 conversion), and the license URL. These notices are included with the installed optional package. No license fee or per-minute inference fee applies to local execution. Release documentation must distinguish the MIT app/runtime from the CC BY model.

The engine's exact bytes and ABI were inspected on 2026-10-31; the model size/digest were read from the pinned repository's LFS metadata. This does not establish that the model has been downloaded, loaded or transcribed successfully in Lyn.

## Download and storage boundary

Keep the existing bounded HTTPS redirect-host allowlist, exact length/SHA-256 checks, private staging, cancellation, extraction rejection and atomic activation. Increase the installation deadline to **20 minutes** for the larger optional download; retain the 15-second connect timeout. Never read remote `latest` at runtime. The engine archive admits only its exact regular library/license/header/document members; no links, path traversal, duplicate members or arbitrary extraction. Retain the verified archive and compare the installed library/license with it when reconciling installation.

Package data remains beneath Rust's application-data directory:

```text
speech/
├── active/parakeet-tdt-0.6b-v3-q8-v1/
│   ├── manifest.json
│   ├── artifacts/engine.tar.gz
│   ├── engine/libparakeet.so
│   ├── model/tdt-0.6b-v3-q8_0.gguf
│   └── notices/
└── staging/<random installation id>/
```

Preserve any previous valid package if installation fails. Existing Whisper package directories remain untouched and are never executed or adopted as Parakeet. A saved speech preference cannot start inference until the Parakeet package is valid. No automatic model migration, download, deletion of old artifacts or cloud fallback occurs.

## Runtime and IPC boundary

Keep the existing safe model states (`not_installed`, `downloading`, `installed`, `invalid`), byte progress and typed errors. Installed means the pinned files and native ABI probe passed, not that a model is currently resident or transcription quality is accepted. Settings identifies Parakeet v3 Q8 and its approximate 942 MB download.

Only database-resolved Lyn-owned 16 kHz mono PCM16 WAV files enter inference. Refuse recordings over ten minutes. Each job, including cold load, has a five-minute absolute deadline; disablement cancels active work. Bound worker requests and replies, never forward native stdout/stderr or log audio/transcript contents. Every generated caption still passes source/revision/manual-caption checks before an enrichment event refreshes Library.

## Acceptance

Automated checks must cover artifact identity, archive traversal/link/duplicate rejection, integrity mismatch, safe old-Whisper handling, worker response bounds, reuse, idle retirement, cancellation, timeout, crash cleanup, one-at-a-time inference, no automatic failed-job replay, manual-caption/revision precedence and the typed Settings/Library consumers. Fixtures establish lifecycle and persistence behavior, not model quality.

Owner testing follows implementation: install through Settings, restart Lyn, record representative French/English notes (accents, technical terms, numbers, noise and silence), inspect editable generated captions and preserved audio, compare cold/warm timing and peak RSS, wait for idle retirement, disable during a job, retry a failed job, remove/reinstall and confirm offline capture. Record installed acceptance separately from local automated checks. No previous Whisper or HEX result establishes Parakeet acceptance.

## Decision history and sources

The original 2026-09-01 choice was whisper.cpp v1.9.2 with multilingual Whisper base, model ID `whisper-base-multilingual-v1`. It was selected for size and CPU compatibility, not measured recognition quality. The 2026-09-29 research recommended target-machine evaluation; the owner now authorizes the replacement before the later joint quality test.

- [NVIDIA Parakeet v3 model card and license](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)
- [Pinned native runtime release](https://github.com/mudler/parakeet.cpp/releases/tag/v0.6.1)
- [Pinned C API and ownership contract](https://github.com/mudler/parakeet.cpp/blob/v0.6.1/include/parakeet_capi.h)
- [Pinned converted model files](https://huggingface.co/mudler/parakeet-cpp-gguf/tree/07a08a3a405d20c5434b9a1442f6e6cf571f57ad)
- [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)
