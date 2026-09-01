# Lyn — G2 Local Speech Distribution Decision

Status: **Accepted for T30 implementation**, 2026-09-01

Derived from: [`02_requirements_srs.md`](02_requirements_srs.md), [`05_architecture.md`](05_architecture.md), and [`06_api_specification.md`](06_api_specification.md)

## Decision

Lyn will offer one optional, multilingual, CPU-first local speech package on the first verified target: Linux x86_64. Installation is an explicit Settings action. Rust owns an immutable allowlist containing engine and model identities, exact origins, sizes, and SHA-256 digests. Downloads use Lyn-owned staging and become active only after complete verification. They never participate in capture acceptance.

| Component | Accepted artifact |
|---|---|
| Engine | `whisper.cpp` v1.9.2, upstream commit `306c88f`, official Ubuntu x64 release asset |
| Model | OpenAI Whisper multilingual `base`, converted to whisper.cpp GGML as `ggml-base.bin` |
| Product model ID | `whisper-base-multilingual-v1` |
| Languages | Automatic multilingual transcription; no translation mode |
| Acceleration | CPU only; no CUDA, Vulkan, OpenVINO, or remote fallback |

G2 is resolved for T30 on Linux x86_64 only. Other targets remain unverified until their engine artifacts, checksums, extraction rules, and runtime behavior are accepted separately.

## Why multilingual base

The OpenAI model card identifies `base` as a 74-million-parameter model and distinguishes multilingual models from `.en` English-only variants. Lyn is used in French and English, so `base.en` is unsuitable as the sole v1 model. whisper.cpp reports approximately 142 MiB of disk and 388 MB of memory for `base`, compared with approximately 466 MiB/852 MB for `small`; `base` is the better first-release balance for a lightweight capture utility.

Whisper output remains advisory metadata. OpenAI documents uneven performance across languages and accents, possible hallucinated text, and repetitive output. Lyn therefore keeps user-caption precedence and revision checks, labels generated text as a transcript, and never uses it for decisions or silently replaces user-authored metadata.

## Immutable artifact manifest

### Engine

- Version/commit: `v1.9.2` / `306c88f`
- URL: `https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-ubuntu-x64.tar.gz`
- Exact size: `9,497,583` bytes
- SHA-256: `46811a3ecf584307480a220b9ef5ff81b7b22dc41577cbc274ce3afc61f753b1`
- Accepted content: `whisper-cli`, required shared libraries, and upstream `LICENSE`
- License: MIT

### Model

- Repository revision: `5359861c739e955e79d9a303bcbc70fb988958b1`
- URL: `https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin`
- Exact size: `147,951,465` bytes
- SHA-256: `60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe`
- License metadata: MIT

The 40-character `SHA` values in the upstream model table are repository object identifiers, not Lyn's artifact-integrity field. Lyn uses the independently verified 64-character SHA-256 above.

Both OpenAI Whisper and whisper.cpp publish MIT licenses, and the Hugging Face model repository declares MIT metadata. T30 retains the engine license with the installed component. G3 must include engine/model attribution in release notices. Neither artifact is committed to this repository or bundled with core Lyn.

## Download and supply-chain requirements

The frontend submits only the product model ID. It cannot submit a URL, checksum, filename, destination, engine argument, or filesystem path.

Rust must:

1. Accept only the build-time manifest entry above.
2. Start from the exact HTTPS origins and allow only bounded HTTPS redirects through an explicit GitHub/Hugging Face delivery-host allowlist.
3. Reject downgrade, credentials, fragments, redirect loops, unknown hosts, oversized content, and mismatched lengths.
4. Use a 15-second connect timeout, 30-second idle timeout, and five-minute installation deadline.
5. Stream into a newly created Lyn-owned staging directory with restrictive permissions while computing SHA-256.
6. Require the exact final byte count and digest before activation.
7. Extract only explicitly allowlisted engine members; reject absolute paths, parent traversal, links, devices, unexpected files, and excessive extracted bytes.
8. Validate the engine with a bounded version probe and the model with a bounded local probe, then atomically rename the complete package into the active location.
9. On cancellation, interruption, mismatch, or validation failure, remove only incomplete staging and retain any previous valid package.

The manifest ships with an application release. Lyn never reads remote `latest`, accepts remote manifest updates, or auto-upgrades an installation. Changing engine, model, URL, size, or digest requires a reviewed source change, automated fixtures, owner validation, and a new product model ID when compatibility or output may change.

## Storage and lifecycle

All paths derive beneath Tauri's application-data directory and remain private to Rust:

```text
speech/
├── active/whisper-base-multilingual-v1/
│   ├── manifest.json
│   ├── engine/
│   ├── model/ggml-base.bin
│   └── notices/
└── staging/<random installation id>/
```

IPC exposes only safe state, model ID, byte progress, and total bytes. It never exposes paths, redirect destinations, process output, raw OS errors, or transcript content.

Only one install/remove mutation runs at a time. Removal refuses while transcription uses the package, then removes it by product ID without altering captures, captions, audio, or enrichment history. Installation does not automatically enable transcription; enabling Local speech remains a separate explicit choice.

## Runtime limits

- Opt-in remains off by default; disabled speech schedules and starts no new work.
- At most one transcription runs at a time.
- The adapter accepts only a database-resolved Lyn-owned WAV record and the active manifest entry.
- Input remains 16 kHz, mono, 16-bit PCM WAV, the format documented by whisper.cpp's CLI.
- Recordings longer than ten minutes are skipped.
- Threads are limited to `min(4, available_parallelism)`.
- Each transcription has a five-minute deadline and cancellation terminates only Lyn's child process.
- Stdout is bounded to 512 KiB and parsed as UTF-8 transcript data; stderr is never forwarded or persisted.
- Generated captions still pass Lyn's nonblank, 500-character, source, and revision checks. Longer raw transcripts do not enter the v1 caption/search contract.
- Missing, invalid, disabled, timed-out, or failed speech marks enrichment skipped/failed without changing the capture.

The upstream 388 MB estimate is an expected floor, not a hard allocation guarantee. T33 must measure peak RSS and latency on the reference machine before release; budget changes require evidence and a policy update.

## Required Settings behavior

Settings shows `Not installed`, `Downloading`, `Installed`, `Invalid`, or a safe failure; the `Multilingual base` label; an approximate 150 MB model download; byte/percentage progress; and valid Install, Cancel, Retry, and Remove actions. The Local speech toggle is disabled until the package is valid. The UI states that capture continues when installation or transcription fails. No model prompt appears in quick capture.

## T30 verification and owner gate

Automated tests cover unknown IDs/fields; origin and redirect rejection; declared/streamed overflow; byte-count and digest mismatch; cancellation and interrupted cleanup; archive traversal/link/device/unexpected-member rejection; atomic replacement; invalid startup state; offline failure; removal in use; disablement; engine timeout/cancellation/output bounds; user-caption precedence; and absence of cloud fallback, raw paths, process-output leakage, or save dependency.

After automation passes, the owner must install through Settings, restart Lyn, record short French and English notes, confirm playback/transcripts, disable/re-enable speech, remove/reinstall the package, and confirm capture remains operational offline and during failed installation. That manual result is the final T30 gate before Phase 6.

## Sources and verification

- [whisper.cpp v1.9.2 release](https://github.com/ggml-org/whisper.cpp/releases/tag/v1.9.2)
- [whisper.cpp quick start, WAV requirement, memory usage, and quantization](https://github.com/ggml-org/whisper.cpp/blob/master/README.md)
- [whisper.cpp model download documentation and matrix](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md)
- [Pinned Hugging Face model repository revision](https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1)
- [OpenAI Whisper model card and limitations](https://github.com/openai/whisper/blob/main/model-card.md)
- [whisper.cpp MIT license](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/LICENSE)
- [OpenAI Whisper MIT license](https://github.com/openai/whisper/blob/main/LICENSE)

On 2026-09-01, both exact URLs were downloaded to temporary files. Their byte counts and SHA-256 digests matched this manifest. No artifact was copied into the repository.
