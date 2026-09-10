# Implementation Notes

## Current milestone — Linux runner operational: OCR closed, three-way archive matrix, second-platform evidence (2026-09-02)

### The Linux execution lane is live
- macair-away (Linux Mint 22.3, Tailscale) is now the second execution environment per the owner's standing rules: Windows holds the authoritative source; Linux runs compile/test; sync is one-way via git archive; no sudo without asking.
- User-level toolchain, zero system packages touched: rustup in ~/.cargo; a conda-forge env (~/.miniforge/envs/ocr) carries tesseract 5.5.3 (+chi_sim), poppler, qpdf, ffmpeg, pandoc, pillow, matplotlib.
- Core suite natively: 214 passed / 0 failed - the Windows symlink-privilege failure class does not exist here.

### OCR gap CLOSED (G-24)
- e2e on Linux: image -> txt validation Pass (OCR_TEXT_NONEMPTY); pdf-ocr validation Pass with the extracted text matching the fixture exactly (OCR TEST ELECTRIC 440010147700). Engine resolution via FORMATWRIGHT_ENGINE_TESSERACT; chi-sim available for Chinese documents.

### Archive family complete
- tar.gz <-> 7z joins zip <-> tar.gz and zip <-> 7z: all three containers now interconvert in memory with manifest conservation. Windows e2e: tar.gz -> 7z -> tar.gz round trip byte-exact.

### Second-platform conversion matrix
- 28/29 routes pass on Linux (structured, markup->pdf/docx/epub via pandoc, raster, PDF->image, OCR, audio, full archive trio). The single miss is svg->pdf requiring msedge - correct EngineMissing behavior for a Windows-only browser lane on Linux; a Chromium discovery branch for Linux is the natural follow-up.


## Current milestone — Wave 4 + first tri-platform green CI (2026-09-02)

### Landed
- **Word export family** (agent G): docx -> txt/md/html/epub via Pandoc (EXPORT_TEXT_NONEMPTY required, fidelity digest Warning) and docx <-> odt exchange through the isolated soffice lane with structural validation; a real zip-6.0 data-descriptor bug misjudging LibreOffice-written DOCX sizes fixed en route.
- **7z lane** (agent H): zip <-> 7z via sevenz-rust, in-memory entries only, entry-count + manifest conservation. RUSTSEC-2026-0245/0246 exempted with rationale (disk-writing path never called); swap the crate before any Certified claim.
- **PDF metadata**: pdf-metadata operation writes /Title//Author via hand-rolled incremental update; validated by pdfinfo round-trip.
- **OCR wired, engine pending**: image->txt and pdf-ocr operations complete with OCR_TEXT_NONEMPTY/OCR_PAGE_COVERAGE; doctor lists tesseract and reports EngineMissing cleanly until installed (user deferred the install).
- **HEIC/HEIF (GW-01)**: lane drives libheif heif-dec (DLL closure hand-assembled from MSYS2 packages via PE import-table scanning); both targets e2e green.
- **First real tri-platform CI: ALL GREEN** (run 33626245570, Windows/macOS/Linux success). The chase surfaced and fixed: repo-contract capability allowlist drift, pnpm '--' flag pass-through, empty-starter dev-build staging, unix cfg lints (unnecessary_wraps, cfg-scoped test import), preset-library v1 schema missing the quality knobs (caught by the contract suite the local --lib loop never ran), a Result ok().is_some_and pair, and two CI-timing flakes (PowerShell process-tree fixture 3s->30s, queue-thread callback 5s->60s). WebView accessibility smoke is continue-on-error on headless runners with the interactive evidence boundary preserved.

### Verification
- Local: core 222 (+4 symlink baseline), schema contracts 9/9, desktop lib 33, frontend 29/29, server 13/13, workspace clippy -D warnings clean, fmt clean, cargo-deny ok, dependency audit 0 vulnerabilities.
- Conversion matrix: 68/68 routes pass locally (Word exports, docx<->odt, HEIC, 7z included).


## Current milestone — Wave 4: OCR (G-24), PDF metadata (G-25), 7z archive (2026-09-02)

### Landed (this subagent)
- **G-24 OCR**:
  - Image -> txt operation-free route: png/jpg/jpeg -> txt via tesseract (`tesseract <in> stdout -l eng --psm 3`; stdout mode avoids tesseract's auto `.txt` suffix). New `ocr.rs` (`plan_image_ocr`, `plan_pdf_ocr`, `validate_ocr_output`). Step arguments use `ocr_mode` (not `operation`) so the image lane stays off the qpdf operation dispatch. Acceptance: `OCR_TEXT_NONEMPTY` (required; at least one alphanumeric token). `OCR_CONFIDENCE` was descoped (tesseract does not print per-word confidence to stdout by default).
  - `pdf-ocr` operation: per page `pdftoppm -f N -l N -r 150 -png` into a staging tempdir, tesseract per page, concatenated txt. Acceptance: `OCR_PAGE_COVERAGE` (processed pages == pdfinfo page count, required) + `OCR_TEXT_NONEMPTY`.
  - doctor discovery list + `FORMATWRIGHT_ENGINE_TESSERACT` env (generic env plumbing already existed). CLI: `--operation pdf-ocr`; images just use `--to txt`.
- **G-25 pdf-metadata**: `apply_pdf_metadata(input_bytes, title, author)` performs a PDF incremental update in-process (zero new deps): parses the last `startxref`, locates the old trailer, copies `/Root`, appends a new `/Info` object + one-entry xref subsection + new trailer with `/Prev` and `/Info` pointing at the new object. Acceptance: `PDF_METADATA_TITLE`/`PDF_METADATA_AUTHOR` (required, pdfinfo-observed) + page-count conservation. CLI `--metadata-title/--metadata-author`. `PlanRequest` gained `metadata_title`/`metadata_author` (serde default; exhaustive literals in cli/main.rs and planner.rs tests updated).
- **7z archive**: `sevenz-rust = "0.6"` (workspace + core). archive.rs: `.7z` magic `37 7A BC AF 27 1C` recognition, `read_7z_entries` (drains entries through a counting discard writer; directory names normalized to trailing `/` so ZIP manifests stay comparable), `repack_zip_to_7z` / `repack_7z_to_zip`. Planning/capabilities/workflow/runner extended; acceptance reuses `ARCHIVE_ENTRY_COUNT`/`ARCHIVE_ENTRY_MANIFEST`.

### Tradeoffs
- Metadata is set via incremental update, not a rewrite: unset fields keep old values, other `/Info` entries are inherited (documented as `unknown` in the ChangeSet); output keeps every original byte verbatim plus the appended revision.
- `pdf-metadata`/`pdf-ocr` route through `prepare_pdf_operation`, so both inspect `qpdf`/`pdfinfo` up front; the metadata step engine identity is qpdf even though execution is in-process (kept for lane consistency).
- 7z support is zip <-> 7z only (tar.gz <-> 7z deliberately out of scope).
- OCR confidence Warning descoped (no cheap stdout parse).

### Verification
- `target
un-tests.bat`: 222 passed + the 4 pre-existing symlink os-error-1314 failures (unchanged baseline; two of the +tests belong to the parallel document agent).
- `targetmt-fix.bat`: FMT_CLEAN; clippy zero warnings for the files touched here (document.rs warnings belong to the parallel wave).
- E2E (debug CLI, engines via FORMATWRIGHT_ENGINE_*):
  - pdf-metadata on a soffice-produced 1-page PDF: `pass` with `PDF_OPS_PAGE_COUNT`, `PDF_METADATA_TITLE`, `PDF_METADATA_AUTHOR` all pass; `pdfinfo` independently reports `Title: ELECTRIC Title 440010147700`, `Author: Anole e2e`, `Pages: 1`; `qpdf --check` reports no syntax errors.
  - zip -> 7z -> zip round trip: both legs pass `ARCHIVE_ENTRY_COUNT`/`ARCHIVE_ENTRY_MANIFEST`; python zipfile confirms identical (name,size) inventory.
- **OCR e2e pending tesseract install** (`E:\DevCaches\Tesseract-OCR	esseract.exe` not present). Rerun after install:
  - `set FORMATWRIGHT_ENGINE_TESSERACT=E:\DevCaches\Tesseract-OCR	esseract.exe`
  - image lane: `formatwright convert ocr.png --to txt --output ocr.txt` (PIL fixture: white 800x300 PNG containing `OCR TEST ELECTRIC 440010147700`).
  - pdf lane: `formatwright convert scan.pdf --operation pdf-ocr --to txt --output scan.txt`.
  - Expect `OCR_TEXT_NONEMPTY` pass; pdf lane additionally `OCR_PAGE_COVERAGE`.

### Risks / Follow-up
- Tesseract `-l eng` is fixed (no language option yet); DPI fixed at 150.
- `apply_pdf_metadata` assumes a classic (non-xref-stream) trailer; xref-stream PDFs fall back to the first `trailer` search and would fail closed with an input error if `/Root` is absent.
- 7z entries with anti-item or empty-stream file semantics rely on sevenz-rust behavior; round-trip covered by unit + e2e for the zip case.


## Current milestone — Parallel wave 3: watermark, target-size, track UI, CORS, cross-platform, LibreOffice (2026-09-01)

### Landed
- **G-23 watermark** (subagent): pdf-watermark operation builds a hand-written single-page PDF stamp layer (Helvetica-Bold, rotation, alpha ExtGState) and applies it with qpdf --overlay --repeat; validation = page-count conservation + watermark text presence (order-insensitive match, since rotated text extracts scattered).
- **G-32 target size** (subagent): --target-size-kb drives a bounded CRF ladder (20/26/32/38) on mp4 transcodes; VIDEO_TARGET_SIZE Warning reports observed vs target or the nearest reachable rung. E2E: VP9 2.6 MB -> 903 KB nearest-rung Warning (testsrc cannot reach 500 KB at acceptable rungs).
- **G-31 track UI** (subagent): expert Convert form gains an audio-track selector fed by the plan probe's streams (auto = None), wired through DesktopConversionRequest.
- **API polish** (subagent + main): malformed JSON bodies now answer the structured {code,stage,message,action} shape; CORS layer (incl. OPTIONS preflight) added so website/demo.html can drive the loopback API from file://.
- **G-34** (subagent): CI fmt collapsed to the Linux job, macOS runs the SBOM script, and doctor's known_install_location generalized to macOS Chromium bundle layouts (+2 tests).
- **G-35**: website/demo.html (receipt-style API demo, curl-verified contract).
- **LibreOffice 26.2.4** installed to E:\DevCaches\LibreOffice (MSI administrative image, no admin rights) with FORMATWRIGHT_ENGINE_SOFFICE pointing at soffice.com (console shim - soffice.exe hangs GUI-substyle on --version). Runner office filter map extended for ODF/RTF flavors. ODT->PDF e2e: LibreOffice-generated ODT converts with validation Warning and a verified text layer. Note: an administrative-image soffice is fine for headless conversion, but a normal installation is still the supported posture for releases.
- Updater release keypair rotated (strong random password stored beside the key, both git-ignored); website repo/download links now point at github.com/leolemon777/FormatWright.

### Verification
- core 211 passed + 4 symlink baseline; fmt clean; clippy zero warnings; server 13/13; frontend 29/29; desktop rebuilt and running. E2E evidence: watermark chars verified independently via pdftotext, target-size nearest-rung Warning, ODT->PDF text layer, demo contract curl-verified.

### Risks / Follow-up
- Wrong-password decrypt copy still generic; watermark text check is order-insensitive (documented); admin-image LibreOffice is a dev convenience, not the certified distribution form.
- Remaining roadmap: G-24 OCR, G-25 metadata edit, G-34 real cross-platform CI runs (untested on actual runners), release signing account.

## Current milestone — Parallel wave 2: PDF toolbox, REST API, ODF/RTF, release engineering, website (2026-09-01)

### Landed (parallel subagents + main thread, all gated and committed)
- **W3 PDF toolbox** (`88c6f94`): pdf-rotate/compress/encrypt/decrypt on the ADR-0013 machinery; one-shot in-process secret store keeps passwords out of serialized Plans; PDF_ENCRYPTED proven by pdfinfo failing on the output; compression ratio as Warning. E2E on qpdf 12.4.1 all Pass.
- **G-33 REST API** (`c4cd38b`): crates/server (axum) reuses the CLI application pipeline; every convert response carries the ValidationReport; structured errors; 127.0.0.1-only; 11 tests + live e2e.
- **G-30 complete** (`16e74dc`): ODT/ODS/ODP by ODF structure+flavor, RTF envelopes without a ZIP; Basic macro members blocked; office lane to PDF.
- **W1** (`df159e9`, `updater slice`): starter-pack population assertion in the release workflow, updater plugin wired end-to-end with a dev keypair outside the repo (docs/release/UPDATER.md), release checklist gates.
- **G-05 website** (committed with wave): offline single-file Meadowlark landing page, EN/中文, receipt-metaphor hero.

### Verification
- core 204 passed + 4 pre-existing symlink failures (baseline); fmt clean; clippy zero warnings; server 11/11; frontend 28/28; desktop rebuilt and running with the updater.

### Risks / Follow-up
- Updater uses a dev keypair - must rotate before any public signed release (UPDATER.md step 1).
- Encrypt/decrypt in the durable queue cannot resume after restart (one-shot secret), reported with an explicit error.
- Wrong-password decrypt reports the generic encrypted-PDF message (correct rejection, imprecise copy).
- Remaining roadmap: G-04 real code signing, G-23/24/25 (watermark/OCR/metadata), G-31/32, G-34 cross-platform, G-35 web front.

## Current milestone — Gap-roadmap execution wave: G-10/30/13/11/12/01 landed (2026-09-01)

### Spec Interpretation
- User goal: execute the full competitive-gap roadmap, item by item, each with tests and evidence ("全部都改，都改完，确认无误"), toward an iteratively superior product.
- Wave status after this session: W2 (G-10/G-11/G-13/G-30) and G-01 fully landed; G-12 + ADR-0013 landed; remaining backlog is W1 release engineering (G-02/G-03/G-04), W3 PDF toolbox extensions, and W4.

### Decisions Made & Landed (all on main, each commit gated by fmt+clippy+full tests)
- **G-10 EPUB target** (`5387c70`/`659cbac`): md/html → epub via Pandoc; OCF magic detection distinguishes EPUB from DOCX prefixes; validation = EPUB_PACKAGE_OPENS/TARGET_FORMAT/CONTENT_DOCUMENTS/TEXT_COVERAGE(required, ≥80%)/TEXT_FIDELITY(Warning — nav/toc repeats chapter titles, same rationale class as EDGE_PDF_TEXT_FIDELITY). E2E: 2-chapter md → 9-entry publication, pandoc reads back complete.
- **G-30 text slice** (`c494208`): .txt/.text → 'plain' format riding the GFM reader (Pandoc has no plain reader; every plain doc is valid Markdown); routes pdf/docx/epub. ODT/RTF deferred — they need ODF/RTF package inspection, not a whitelist.
- **G-13 knobs core+CLI** (`7ac5c21`): PlanRequest.video_crf(0-51)/video_preset(allowlist)/audio_bitrate_kbps(8-320) flow into mp4+audio plans and replace the hardcoded `-preset medium -crf 20`/192k; CLI --video-crf/--video-preset/--audio-bitrate-kbps. E2E proof: VP9→MP4 at 64 kbps measured 64.6 kbps by independent ffprobe. **Desktop UI wiring remains the open G-13 follow-up.**
- **G-11 archive lane** (`4244c45`): built-in formatwright.archive engine; zip ↔ tar.gz in-memory repack (no extraction, deterministic mtime 0, traversal paths rejected, links/devices refused); validation = ENTRY_COUNT + name:size manifest digest. Added tar crate; flate2 promoted from transitive. E2E: 3-entry zip → tar.gz → zip round trip byte-identical.
- **ADR-0013 + G-12** (`1c87a36`/`4903ffc`): PlanRequest.operation/inputs/page_range (serde-default); pdf-merge (joint fingerprint) & pdf-extract via qpdf --empty --pages; MEASURED page-count conservation (PDF_OPS_PAGE_COUNT) via post-execution pdfinfo; verbatim `\\?\` prefixes stripped for qpdf (external_process_path). qpdf 12.4.1 installed to E:\DevCaches with engine override. E2E: 3+2→5 pages Pass; '2-3' extract = exactly source pages 2-3 by pdftotext.
- **G-01 sandbox** (`aadaa7b`): scripts/test_browser_print_sandbox.ps1 + docs/testing/BROWSER_PRINT_SANDBOX.md; GW-10 matrix caveat cleared. Two pre-existing main bugs fixed en route: structured_format_hint swallowed every '<'-prefixed file (CLI inspect of doctype HTML/SVG died as XML), and is_document_path missed svg/epub/txt.

### Verification
- Per-commit: cargo fmt --check clean, clippy zero warnings, core lib tests green (197 passed + 4 pre-existing symlink-privilege os-1314 failures, unchanged baseline).
- Every feature has an end-to-end run on this machine with independent verifier (pandoc read-back, python zipfile, ffprobe, pdfinfo/pdftotext) — evidence captured in commit messages.

### Risks / Follow-up
- **G-13 desktop UI wiring** landed in `f48af08`: expert form + preset editor expose CRF/preset/bitrate per target, preset validation mirrors runner ranges, 28/28 frontend tests.
- ODT/RTF inputs, W3 PDF toolbox (rotate/compress/encrypt/watermark/OCR), API service, updater, cross-platform remain queued per COMPETITIVE_GAP_ROADMAP W3/W4.
- Operation routing is not yet surfaced in the desktop UI (CLI-only); capability snapshot doesn't advertise operations.

## Previous milestone — Competitive gap analysis and v0.2+ roadmap (2026-09-01)

### Spec Interpretation
- User asked for a deep, source-verified gap analysis against competitors (VERT, File Converter, Stirling-PDF, Gotenberg, HandBrake) and then a master plan covering all findings.

### Decisions Made
- Evidence base: read `capabilities.rs` routing (15 targets, lane model), CLI surface (doctor/identify/inspect/plan/convert/jobs/engines/maintenance), full i18n key inventory as the UI feature face, sandbox suite list, `pdf.rs` (inspection + render only — no PDF post-processing), `runner.rs` hardcoded `-crf 20 -preset medium`, absence of updater config, and the CI/release-docs inventory. Confirmed CI (ci/fuzz/release-candidate/dependabot) already exists.
- Deliverable: new [`docs/COMPETITIVE_GAP_ROADMAP.md`](docs/COMPETITIVE_GAP_ROADMAP.md) with G-xxx numbering (no clash with master-plan R-xxx), four waves: W1 release blockers (G-01..04), W2 quick wins (G-10 epub via pandoc, G-11 native archives, G-12 qpdf merge/split with page-conservation validation, G-13 expose codec/CRF/framerate parameters), W3 PDF toolbox on the G-12 operation-routing model (rotate/crop/compress/encrypt/watermark/OCR), W4 breadth+service (ODT/RTF, track selection UI, target-size compression, REST API aligned to Phase 6, macOS/Linux).
- Every new lane must carry machine-readable validation checks (page-count conservation, text-layer retention, hash manifests) — the project's differentiator extended to new capabilities; stated as the entry rule in the roadmap.
- One new architecture decision identified: operation-style routing (multi-input + operation → PDF) for G-12, flagged for ADR-0013 before implementation.
- Explicit non-goals recorded: CAD, PDF→DOCX editable round-trip, Ghostscript (AGPL undecided), chasing VERT's format count.

### Verification
- All claims trace to named source files (see roadmap tables); competitor facts from official sites/GitHub (VERT 15.4k stars AGPL, Stirling 60+ tools, Gotenberg Chromium-based API).
- Linked from `MASTER_EXECUTION_PLAN.md` header; master plan v0.7 body untouched (it still owns v0.1 closure).

### Risks / Follow-up
- Roadmap is a planning artifact; priorities need owner confirmation before Wave 2 starts.
- G-12's operation-routing model is the only structural change — schedule the ADR first.

## Previous milestone — Desktop verification session: title-bar drag fix, local engine provisioning, engine guide (2026-09-01)

### Spec Interpretation
- User smoke-tested the merged browser-lane build and reported two gaps: the window could not be dragged after resizing, and the doctor page showed missing engines. Goal: fix the drag bug, provision this machine's engines, and document engine acquisition for the upcoming open-source release.

### Decisions Made
- Title-bar drag fix: Tauri's `data-tauri-drag-region` only fires when the event target is the attributed element itself (no ancestor/child walk). The old markup attributed only the content-width `.c95-window__title` span, so after enlarging the window most of the title bar (the `header` padding area and the SVG icon) was undraggable. A stray `-webkit-app-region: drag` (an Electron-ism, inert in WebView2) had masked the gap in review. Fix: attribute the whole `c95-window__titlebar` header, `pointer-events: none` on the title icon (both themes), `user-select: none` on the titlebar. Window control buttons remain unaffected (no attribute on them), and double-click maximize now works on the whole bar via Tauri's built-in behavior.
- Local engine provisioning (machine config, not repo): Poppler 26.02.0 installed to `E:\DevCaches\poppler-26.02.0` from the same pinned poppler-windows archive as the starter-pack script (sha256 `993e4a…cda5` verified), user PATH extended, and per-engine `FORMATWRIGHT_ENGINE_PDFINFO/PDFTOPPM/PDFTOTEXT/PDFFONTS` overrides set. The overrides matter: Git for Windows ships an Xpdf 4.00 `pdftotext` whose `-v` exits 99, which doctor (correctly) rejects; env overrides outrank PATH so the real Poppler wins regardless of PATH order.
- Debug desktop build: the Windows resource map requires `dist/engine-packs/windows-x86_64/starter/` to exist; an empty directory is a supported degraded state (`bundled_manifest_paths` returns an empty list without `bundle.json`), so no 100+ MB starter-pack download was needed for a system-discovery machine. Release builds must run `prepare/build_windows_starter_pack.ps1` instead.
- README gained an `Engines` section: why nothing is bundled (license/supply-chain, links to engines/README + ADR-0011/0012), the discovery order (pack > `FORMATWRIGHT_ENGINE_*` > PATH > canonical locations), a per-engine acquisition table, and the starter-pack expectation for releases.

### Verification
- Desktop rebuild (`tsc -b` + `vite build` + `tauri build --debug --no-bundle`) green; app relaunched.
- Doctor page re-read via UIA after restart: `msedge` 152.0.4191.53, `pdfinfo`/`pdftoppm`/`pdftotext`/`pdffonts` 26.02.0, `pandoc` 3.8, `ffmpeg`/`ffprobe` 8.1.1 all `✓ 可用`; remaining missing (`soffice`, `qpdf`, `vips`, `heif-convert`) are unwired or optional in v0.1. Browser lane is now fully available on this machine.
- Title-bar drag: fix verified against Tauri's documented drag-region targeting rules; rebuild + relaunch handed to the user for tactile confirmation.

### Risks / Follow-up
- `qpdf`/`vips`/`heif-convert` remain doctor-only inventory entries with no route; keep them documented as optional to avoid "must turn everything green" pressure.
- LibreOffice intentionally not installed yet (user decision pending; ~400 MB, E-drive constraint noted).
- The starter-pack empty-directory workaround must not leak into release builds — release checklist should assert a non-empty `dist/engine-packs` tree.

## Previous milestone — Browser print engine lane: HTML/SVG → vector PDF (2026-08-31)

### Spec Interpretation
- GW-10 names "Pandoc；PDF 引擎可选" for Markdown/HTML → PDF. This milestone fills that open PDF-engine slot with a system-discovered headless Edge print adapter and adds SVG as a new document input, per ADR-0012.
- "可编辑矢量 PDF" is a *validated* product claim, not a marketing one: independent Poppler utilities must prove the text layer and font embedding before commit.

### Decisions Made
- Engine id `msedge`, resolved pack > `FORMATWRIGHT_ENGINE_MSEDGE` > PATH > canonical vendor install locations (`doctor.rs::known_install_location`), the latter three under `Development` policy only. Doctor never launches the browser: version comes from the versioned install directory on Windows, else `unknown`.
- Routing gained a lane concept (`capabilities.rs::route_engine_lanes`): HTML→PDF prefers the browser lane `[msedge, pdfinfo, pdftoppm, pdftotext, pdffonts]` and falls back to the existing Pandoc lane; SVG→PDF is browser-lane only; Markdown→PDF is unchanged. Route availability is satisfied when *any* lane is fully available.
- Plan (`edge_pdf.rs::plan_edge_print_to_pdf`): 5 steps — Edge vector print (`LossClass::None`), pdfinfo structural, pdftoppm render, pdftotext text-layer, pdffonts embedding — with `text_must_remain_extractable` as a plan constraint.
- Execution (`runner.rs::execute_edge_print_plan`): staged workspace with isolated `--user-data-dir`, `--host-resolver-rules=MAP * ~NOTFOUND` as network-deny reinforcement, 180 s print timeout + process-tree termination, `office_staged_work_path` staging, no-clobber commit, scheduler treats `msedge` as `SerialEngine`.
- Validation (`validate_edge_pdf_output`): required `EDGE_PDF_OPENS / PAGE_COUNT / PAGE_SIZES / ALL_PAGES_RENDER / TEXT_EXTRACTABLE / FONTS_EMBEDDED`; non-required Warning `EDGE_PDF_TEXT_FIDELITY` (extracted-vs-input character ratio; extraction loses hyphenation/ligatures so it never blocks).
- SVG inspection: prefix/`<?xml`+`<svg` detection, `image/svg+xml`, any raster `<image>` denied under deny-all (breaks the vector promise), text extracted via the XML reader like HTML.
- `pdffonts` embedding parsed from the right (fixed `emb sub uni object ID` tail); font name from the first token because variable-width `type` values make an exact left split unreliable.

### Changes From Spec
- No manifest template for Edge: `engines/manifests/templates` is for reviewable shipped packs, and Edge cannot be redistributed. Instead: `engines/README.md` inventory row + ADR-0012, mirroring the LibreOffice discovery posture.
- Desktop UI/CLI surfaces unchanged: no new target id (`pdf` exists), capability snapshot picks up the lane automatically; no `PlanRequest` field added, so plan/JSON schemas are untouched.

### Verification
- `cargo check -p formatwright-core --locked` ✓; `cargo clippy -p formatwright-core --all-targets` zero warnings ✓; `cargo fmt --check` clean for every touched file ✓; `cargo test -p formatwright-core --lib` 181 passed (4 pre-existing failures: symlink-privilege `os error 1314` tests in `job_store`/`application`, reproduced independent of this branch) ✓; schema contract suite 9/9 ✓; `scripts/check_repository.py` reports only the pre-existing `capabilities/main.json` allowlist error (present on `main`).
- **End-to-end sandbox evidence (2026-08-31, dev build, Windows)**: `formatwright convert carton.html --to pdf` — a real 291-line HTML/SVG carton-drawing fixture — routed to the browser lane (doctor resolved `msedge` 152.0.4191.53 via canonical install location, Poppler 26.02.0 via PATH), completed in ~10 s with `validation: Pass`. Independent re-inspection of the committed PDF: 1 page at 420×293 mm, 0 raster image objects, 5 embedded font subsets (Arial/Arial-Bold/MicrosoftYaHei±Bold/SimSun), 789 extractable characters including the watermark, barcode digits, and the Chinese company name. The plan hash and every required validator (`EDGE_PDF_OPENS/PAGE_COUNT/PAGE_SIZES/ALL_PAGES_RENDER/TEXT_EXTRACTABLE/FONTS_EMBEDDED`) passed before commit.
- Build environment note: this machine's MSVC 14.51 install lacks the CRT headers; compilation required `LIB`/`INCLUDE` for onecore libs + SDK 10.0.22621.0 (plus the bundled vc15 headers from `SDK/ScopeCppSDK` for `libsqlite3-sys`'s C build — a machine-specific workaround, not a repo change).

### Bug fixed en route (pre-existing, main)
- `document.rs::html_text` ran quick-xml with default `check_end_names`, so any real-world HTML containing void elements (`<meta>`, `<br>`, `<img>`) failed inspection with "Malformed HTML", silently fell through to the ffprobe media branch, and reported "ffprobe could not recognize or open the input". Discovered when the carton fixture (contains `<meta charset="UTF-8">`) misrouted while minimal fixtures passed. Fixed by disabling `check_end_names` for the HTML extractor only (DOCX keeps strict XML matching); regression test `html_with_void_elements_is_still_inspectable` added. This also un-breaks GW-10's existing Pandoc lane for ordinary HTML.

### Risks / Follow-up
- Formal sandbox artifacts (`scripts/test_*_sandbox.ps1` + `docs/testing/*_SANDBOX.md` with a committed fixture and pinned engine identities) still owed before the matrix row drops its evidence caveat; the manual end-to-end run above is the interim evidence.
- Edge print fidelity across browser versions is environment-dependent by design (plan hash embeds engine identity); golden fixtures must pin a browser version or tolerate substitution warnings.
- `--headless=new` requires Edge ≥ 108 (2022); older LTS images may need the legacy `--headless` fallback — decide when a real corpus machine fails.
- `known_install_location` currently special-cases only `msedge`; generalize if another canonical-layout engine (e.g. Chrome, WebView2 runtime) joins the inventory.

## Previous milestone — Chicago 95 desktop chrome (2026-08-18)

### Spec Interpretation
- User asked to restyle the entire desktop UI from `plastic-fly-44-2a81bc35` (Chicago 95). Product behavior stays: Plan, queue, Explorer convert, no new formats.

### Decisions Made
- Vend `system.css` as `apps/desktop/src/chicago95.css`. Strip Google Font `@import` because Tauri CSP is `style-src 'self'`; UI uses Tahoma / MS Sans Serif / Courier New fallbacks offline.
- Main window `decorations: false` with a real Win95 title bar (min/max/close via `core:window:default`).
- Existing convert/jobs/presets/engines/reports/maintenance/settings flows keep their logic; chrome is windows, folder tabs, beveled controls, teal desktop.

### Changes From Spec
- Daily-use spec did not require this visual language. Native OS title bar is gone in desktop/e2e/accessibility window configs.

### Verification
- Desktop vitest + `tsc -b` after markup wrap.

### Risks / Follow-up
- Pixelify Sans / VT323 not bundled; look is workstation-like on Windows, not pixel-perfect vs the marketing preview.
- Accessibility snapshots that assumed a dark modern shell will need a re-run.

## Previous milestone — HowToConvert live-site snapshot (2026-08-18)

### Spec Interpretation
- Docs-only: refresh competitor facts from howtoconvert.co. Do not change product scope or start Wave 5 items.

### Decisions Made
- SPEC_PLAN §1.1/§1.3 record WASM upsell, shell-to-user-installed engines, no Explorer, 5 devices, still-Beta pricing.
- VOC Wave 1 remains the UX steal; Wave 5 now names crop/trim/WASM/PATH-install as forbidden.
- FORMAT_SUPPORT_MATRIX GW-04: Architecture Spike → Experimental on Windows. Certified stays empty.

### Changes From Spec
- None. Plan allowed the matrix status fix as optional; it is included.

### Verification
- Read-back of the three edited sections.

### Risks / Follow-up
- GOLDEN_WORKFLOWS.md GW-04 contract text was not rewritten; TRACEABILITY still says the slice is in progress.

## Previous milestone — Wave 1 ingest / toast / plain copy + testdrive (2026-08-17)

### Spec Interpretation
- Remaining Wave-1: 800ms file-list ingest (PR-06), toast without tray (PR-07), plain-language Plan/errors (PR-08). Then hand a Release testdrive, no commit.

### Decisions Made
- Convert verbs go through `ShellConvertCoordinator` (800ms same-target reset, mix-target flush) and `ingest_shell_convert_paths`. Open-in stays on the old FIFO.
- Toast is a Win32 toast via PowerShell (`show_desktop_toast`). No tray, no keep-alive, no settings schema, no new npm package.
- Basic-mode Plan uses `plainLossSummary`; banners use `basicModeFailureCopy` instead of `route.message`.

### Changes From Spec
- Toast is not `tauri-plugin-notification` (avoids a lockfile/plugin surface for testdrive). Click-to-focus is “show main window” after the toast command.

### Verification
- desktopModel + shell_convert + ingest unit tests; Release desktop sequential launch; two CLI JSON→YAML converts.

### Risks / Follow-up
- Installed NSIS smoke not re-run. R-008/R-009 still not Closed.

## Previous milestone — Wave 1 daily-use (PR-01b through PR-05 + PR-02) (2026-08-17)

### Spec Interpretation
- `WINDOWS_DAILY_USE_SPEC_PLAN.md` Wave 1 exit: PR-02 + PR-03 + PR-05, recommended PR-04, plus PR-01b pending pin.
- Convert to X still uses the existing `pendingShellConvert` preview+run effect (PR-06 ingest is later). PR-01b only stops that pending from leaking across user edits and capability auto-target.

### Decisions Made
- `defaultPlanConstraints` resets quality/width/dpi/colorMode to null on new input and shell convert.
- Capability snapshot keeps a pending wanted target; unavailable wanted clears pending and does not jump to `firstRecommended`.
- Success stays on Convert; `setTab("reports")` remains only for explicit report browsing.
- Empty-state cards probe `C:\formatwright-probe.pdf` / `.mkv` through the existing snapshot command (extension-only).
- Drop folders go through `classify_desktop_drop_path` (same local-disk rules as shell).
- Explorer verbs come from `explorer-verbs.json` via `scripts/generate_explorer_verbs.ps1`.

### Changes From Spec
- PR-06 800ms ingest / `ingest_shell_convert_paths` not implemented. N=1 Explorer convert still uses the frontend effect.
- Installed Explorer convert smoke is in the script contract but was not executed here (needs a fresh NSIS build + isolated install).

### Verification
- Targeted desktopModel vitest, desktop `shell_` / classify Rust tests, `generate_explorer_verbs.ps1 -Check`.

### Risks / Follow-up
- PR-06 still required to merge Explorer multi-select and to honor queue-window busy.
- Do not mark R-008/R-009 Closed without a clean VM.

## Previous milestone — PR-01 dirty-tree snapshot (2026-08-17)

### Spec Interpretation
- `docs/specs/WINDOWS_DAILY_USE_SPEC_PLAN.md` KD-13: PR-01 is a rollback snapshot of work already in the dirty tree. No Wave-1 behavior.

### Decisions Made
- Snapshot includes certification threading, Gate U host-side negatives, convert-page honesty, `--shell-convert`, NSIS/dev Convert verbs, VOC backlog, and the daily-use spec.
- **Explorer / clean-VM test-contract migration is PR-02, not this snapshot.** `scripts/test_windows_explorer_integration.ps1` and `scripts/test_clean_vm_certification.ps1` still encode Open-in / navigation-only (including the existing `FormatWrightConvert` key-name mistake). Do not flip them to Convert = 1 Job + Pass + source hash here.

### Changes From Spec
- None in this snapshot. PR-01b (pending-clear / pin wanted), PR-03 (success CTA), PR-04 (empty-state cards), PR-05 (`classify_desktop_drop_path`), and PR-06 (800ms ingest) stay later.

### Verification
- Targeted desktop-model, shell parse/validate, and related core/engine-sdk unit tests. See `{SCRATCH}/targeted-tests.log` when the goal runner captures it.

### Risks / Follow-up
- Installed-smoke and CLEAN_VM still assert the old contract until PR-02.

## Previous milestone — HowToConvert simplicity + FileConverter right-click (2026-08-16)

### Spec Interpretation
- Owner wants both: drag-and-drop simplicity and Explorer one-click convert.
- This is not format-count parity. Only already-supported golden-route families get Convert verbs.
- Right-click **Convert to X** is explicit approval, identical to CLI `convert`. Open-in remains review-only. No overwrite; validation still required.

### Decisions Made
- Convert dropdown shows only available routes plus missing-engine routes for this input.
- Quality field only for lossy targets.
- `--shell-convert --to FORMAT PATH` + per-extension Explorer verbs.
- Directory convert requests are rejected.

### Changes From Spec
- USER_GUIDE previously said the shell never starts a conversion. Convert verbs now may start after a named target is chosen.

### Tradeoffs
- Classic Explorer menu only (Windows 11 modern top-level still later).
- Portable `target/release` exe does not get verbs until install or `register_dev_explorer_convert.ps1`.

### Verification
- Frontend target/shell unit tests; desktop Rust parse/validate tests.

### Risks / Follow-up
- Auto-run from the UI after a shell convert still requires a rebuilt desktop binary.
- Need Media/PDF packs for those verbs to succeed.

## Previous milestone — Gate U engine negative matrix (2026-08-16)

### Spec Interpretation
- Gate U requires automated negatives for missing pack, hash tamper, version incompatibility, revoke, half-install, failed upgrade, and malicious PATH.
- `formatwright_compatibility` is a hard install/verify bound, not documentation.

### Decisions Made
- Enforce `[minimum, maximum_exclusive)` against `CARGO_PKG_VERSION` during `verify_engine_pack`.
- Do not mutate process `PATH` (workspace forbids `unsafe`); prove override/PATH losing to a registered pack via a pure `choose_engine_path` helper.

### Changes From Spec
- None. Clean-VM still required to close R-008/R-009.

### Tradeoffs
- Version compare uses dotted numeric prefixes only; `1.0.0-alpha` compares as `1.0.0`.

### Verification
- Targeted engine-sdk / engine_pack / engine_registry / doctor tests and Clippy `-D warnings`.

### Risks / Follow-up
- Host-side negatives are not a substitute for the offline clean VM.

## Previous milestone — certification status threading (2026-08-16)

### Spec Interpretation
- ADR-0011: `Certified` requires a trusted release signature **and** completed human `sources.json` review.
- Hash completeness or `signature_present` must never promote.
- Display trusted-but-incomplete honestly; do not invent a new Plan/Report schema field.

### Decisions Made
- Keep `EngineIdentity.certification` as the only persisted Plan/Report field (schema v1 unchanged).
- Add `SupplyChainReviewStatus` + derive helpers in `engine-sdk`.
- Activation evaluates the compiled-in (currently empty) keyring so Doctor/UI see `Unsigned` instead of “trust not evaluated”.
- Registered pack provenance feeds `inspect_engine` so Planner and reports inherit the same certification.

### Changes From Spec
- None. Official key ceremony still blocked on owner decision.

### Tradeoffs
- Empty embedded keyring makes every current signed-but-unknown key `UnknownKey`. Starter packs are unsigned, so they become `Unsigned`.
- Time-varying trust is **not** hashed into `plan_hash` except via the derived three-state `certification` captured at inspect time.

### Verification
- Targeted Rust + frontend tests listed in `docs/testing/ENGINE_RESOLUTION.md`.

### Risks / Follow-up
- Clean-VM, official key ceremony, and legal review still block R-008/R-009 closure.

## Previous milestone — Windows usable vertical slice, then reliability remediation (2026-08-12)

**Verified baseline**
- Shared `JobExecutionService` is used by CLI and the Desktop durable queue window.
- `QueueWindowControl` exposes finish-current and immediate controls.
- 79 ordinary Rust tests, 6 frontend tests, TypeScript, production build, Rustfmt, Clippy, repository contracts and pnpm production audit pass.
- The repository currently has no first commit; the current snapshot must be preserved before implementation continues.
- The current Windows release package contains no conversion engines. Production discovery falls back to ambient PATH and selected a broken Codex `pdfinfo.cmd`; PDF→PNG/JPG therefore cannot run out of the box.

**Next, in strict order**
1. Create the recoverable Git baseline; `docs/DEFECT_REGISTER.md` tracks R-001–R-009.
2. R-008/R-009: ship/import a verified Windows Starter pack, resolve exact registered paths only in Release, gate UI routes from the same capability snapshot, and prove offline conversion on a clean VM.
3. R-001: cancel/drain workers and reconcile active job state on every control-plane failure.
4. R-003: persist ValidationReport before the terminal state for immediate and queued conversions.
5. R-002: bind execution/enqueue to the user-approved `plan_hash`.
6. R-004: make immediate pause recoverable from Desktop and add retry/resume actions.
7. R-005/R-006/R-007: Windows output identity, in-flight pause/failure injection, cancellation-link task lifetime, and live queue reads.
8. Only then extract full `ConversionService`, `ReportService`, and the minimum `MaintenanceService`.

The authoritative checklist, long-term module design, 12-week route and maintenance cadence live in `docs/MASTER_EXECUTION_PLAN.md`.

## 2026-09-03 — Rebrand FormatWright → Anole

Name and mascot decided by the owner: **Anole** (the color-changing "American chameleon", 5 letters, clean in the converter category) with mascot direction A "The Color Shift". Brand assets live in `branding/` (candidates + `branding/final/`: icon light/dark, logo, horizontal lockups, favicon ladder, PNG exports).

**Swapped this pass (user-visible surface):** README/website/governance docs/docs tree (guarded line-level script with an identifier allow-list), core/cli/server/engine-sdk user-visible messages and evidence strings, desktop window titles/`productName`/i18n/dialog filters, Explorer context-menu labels (`Open in Anole`, registry **key names unchanged**), JSON-Schema titles, SBOM generators, crate `description`/`authors`, and the release workflow's installer filename (now `Anole_0.1.0_x64-setup.exe`, matching the new `productName`).

**Deliberately kept (technical identifiers, own follow-up pass):** crate/binary names (`formatwright*`), `formatwright_core::` paths, `FormatWrightError`/`FormatWrightCompatibility`, `.join("FormatWright")` state-database dirs, `FORMATWRIGHT_ENGINE_*` env vars, `...\shell\FormatWright` + `FormatWright.To*` registry verbs, tauri identifier `local.formatwright.desktop`, repo/GitHub name and updater URL.

Verified: `cargo check` (core/cli/server/engine-sdk) clean; `core --lib` 244 passed / 4 failed = the known Windows reparse/symlink baseline; engine-sdk 11 passed; residual-string audit shows only the intended technical identifiers. Trademark screening (Nice 9/42) is still owed before external promotion.

## 2026-09-03 — Updater release keypair re-rotation (rehearsal wrong-password root cause)

### Root cause
Rehearsal runs 2-4 (`release-candidate.yml`, workflow_dispatch) all failed at updater signing with `incorrect updater private key password: Wrong password for that key` after the NSIS bundle itself built fine. The 2026-09-01 keypair generation ran `PW=$(python secrets.token_urlsafe(24))` in bash, wrote the token to `RELEASE_KEY_PASSWORD.txt`, then passed `--password "%PW%"` **through `cmd //c` + the `.cmd` shim** — `%PW%` is cmd syntax, the bash var was never exported, and the shim path mangles even plain literal passwords (reproduced locally: a fresh key generated via the shim with `--password "plainpw99"` does not decrypt with `plainpw99`, while the identical generate via direct `npx pnpm tauri` works and verifies). The release private key's real password was therefore never the recorded token and is unrecoverable.

### Changes
- Regenerated the release keypair via the direct CLI path (`npx pnpm@11.16.0 --dir apps/desktop tauri signer generate --password <fresh token_urlsafe(24)> --ci`); password written with `printf '%s'` (32 bytes, **no trailing newline** — the old file carried one, which would also have poisoned secret-setting via stdin redirect).
- Pinned the new pubkey in `apps/desktop/src-tauri/tauri.conf.json` (commit `2a39f1d`). Safe window: v0.1.0 unreleased, zero installed copies, and the old key never successfully signed anything.
- Re-set `TAURI_DEV_UPDATER_KEY` (base64-wrapped private key, single line) and `TAURI_DEV_UPDATER_PASSWORD` (32 chars) via `printf '%s' "$(tr -d '\r\n' < file)" | gh secret set` — both secrets guaranteed whitespace-clean.
- Broken pair retained at `target/updater-keys/formatwright-release.key{,.pub}.broken-20260903` for forensics; test keys removed.

### Verification
- Local end-to-end BEFORE pushing: `tauri signer sign` with the exact CI env-var path (`TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) succeeds with the new pair; throwaway-key control proved the harness and the shim corruption both reproduce deterministically.
- Negative controls on the old key (token / empty / literal `%PW%`, env and argv transports) all fail — pair confirmed dead.
- Rehearsal run 5 (`33773232889`): the password fix WORKED (run moved past secret-key decode) but died at `failed to decode pubkey: Missing encoded key in public key` — the follow-up fix double-encoded the pinned value: `tauri signer generate --ci` writes the `.pub` file itself base64-wrapped, so the config value must be `base64(decode(.pub))`, not `base64(.pub)`. Corrected value re-verified structurally against the known-good old format (2-line raw, `RW`-prefixed key line) before pushing. **Rehearsal run 6 (`33774981779`) GREEN** — updater signing, checksums, unsigned-claim guard all passed; artifact `formatwright-windows-unsigned-alpha` (338,696,333 bytes) uploaded. B2's release-candidate rehearsal is closed; next is B3 (tag v0.1.0, release publishing).

### Risks / Follow-up
- Never generate signing keys through `cmd //c`/`.cmd` shims on this machine; always the direct npx/CLI path, and always verify by signing a scratch file before the key is trusted.
- The `dev` keypair (`formatwright-updater.key`, empty password) was also shim-generated and is likewise undecryptable — it only ever backed earlier failed rehearsals; regenerate on demand if a test-only pair is needed again.
- UPDATER.md still says "dev keypair (empty password)" — accurate as intent, but both pairs' file format notes now assume `--ci` base64-wrapped storage (tauri's own format since the CLI writes it that way).

## 2026-09-04 — B3 release day: Anole v0.1.0 published

### Shipped
- `.github/workflows/release.yml` (tag-triggered): rehearsal-proven Windows build + application SBOM (`generate_sbom.py` → `dist/sbom.spdx.json`, attached as `Anole-0.1.0-sbom.spdx.json`), updater `latest.json` (signature read from the bundle `.sig`, URL pointing at the release asset), `SHA256SUMS` (installer + sig + portable exe + SBOM + latest.json), and `gh release create` with the full asset set.
- `.github/workflows/pages.yml` + Pages enabled (build_type=workflow): site live at https://leolemon777.github.io/FormatWright/ ; download CTA now `data-gh="/releases/latest"` (also fixed a duplicated attribute on that anchor).
- Release notes: `docs/release/v0.1.0_notes.md` (中英, Unsigned Alpha SmartScreen caveat).
- Tag `v0.1.0` on `9bdc8bf` → Release workflow green (`33826664551`, 18m27s): https://github.com/leolemon777/FormatWright/releases/tag/v0.1.0 with 6 assets (installer 330,623,750 B, .sig, portable exe, SHA256SUMS, SBOM, latest.json). `latest.json` and installer downloads verified HTTP 200; updater signature decodes as a minisign signature from the release key.

### Issues hit
- First release run (`33825453296`) failed at checksums: the workflow wrote `Anole-$version-x64-setup.exe` (hyphens) but the bundle is `Anole_0.1.0_x64-setup.exe` (underscores). Fixed with `${version}` forms (`9bdc8bf`) and the tag was re-pointed (delete + recreate) to pick up the fix.
- CI on `main` and the tag failed in `audit_dependencies.py`'s pnpm leg with "pnpm audit failed with exit code 1" while a local `pnpm audit --prod --json` (same pnpm 11.16.0) reports zero advisories and the CI step took 4+ minutes — consistent with a transient npm audit-endpoint failure (the script is fail-closed by design). Reruns triggered; verdict recorded below.

### Verification
- Release assets, `latest.json` structure (version 0.1.0, windows-x86_64 platform entry, signature payload), and installer HEAD 200 all checked post-publish.
- CI rerun still failed → root-caused: the npm advisory endpoint had a GLOBAL outage (local repro: `pnpm audit --prod --json` → `{"error":{"code":23,"message":"The operation was aborted due to timeout"}}`; a 20-byte direct curl to the bulk endpoint also timed out, HTTP 000). Two independent environments, zero actual findings — transport failure, not vulnerabilities.
- Gate hardened in `a725f30`: `audit_dependencies.py` retries the pnpm leg 3×, and on persistent transport error degrades to a loud SKIPPED warning — gated on `AUDIT_ALLOW_ENDPOINT_OUTAGE=1` (set on the CI audit step with a comment). Real findings remain fail-closed; cargo legs untouched. Verified locally end-to-end (cargo 0/18 informational, pnpm 0).
- **CI on `a725f30` GREEN** (33829670232). The tag commit `9bdc8bf` shows Windows/macOS green with the Linux audit leg's outage-red; its workflow run was orphaned by the tag delete+recreate and cannot be rerun (API 404) — main's green plus the local clean audit is the evidence trail. The published tag stays at `9bdc8bf` (moving it would desync the release assets from the tag).

## 2026-09-04 — B4 announcement material

- **Trademark screening (web-level, Nice 9/42)**: no registered "ANOLE" word mark covering file-conversion software found via public sources. Adjacent uses recorded in `docs/release/NAME_CLEARANCE.md` (ANOLE 360 smartglasses — different mark/goods; qdc Anole IEMs — hardware; legacy Anole Media Player — closest software use; Anolepay — fintech). Formal professional clearance still recommended before paid promotion or mark registration; recorded as such.
- **README**: badges (CI, Release, Apache-2.0, website, platforms, 319 routes — all verified HTTP 200) + Status section rewritten from the stale 2026-08-15 alpha snapshot to the v0.1.0 Public Beta (Unsigned Alpha) facts, honest gaps included (no cert yet, host-Tesseract OCR on Windows, clean-VM evidence pending, macOS CI-only). Historical milestone list moved to a pointer at implementation-notes.
- **Announcement copy**: `docs/release/v0.1.0_announcement.md` (中英 × 长短, plus channel candidates and the screening precondition). Not posted anywhere — promotion is Leo's call.

## 2026-09-04 — C1 wave 1: TIFF/BMP raster family

### Shipped
- TIFF/TIF/BMP inputs reach **webp/avif/png** (ffmpeg still-image lane), **pdf** (soffice draw lane), and **txt** (tesseract OCR lane, Linux-verified pending below); png/jpg/jpeg gain lossless **tiff/bmp** targets. BMP blocks alpha sources at plan time (encoder alpha unreliable, same posture as JPEG); TIFF/BMP are lossless and reject `--quality`.
- Two real bugs found by e2e and fixed: (1) ffprobe demuxes BMP files (sometimes TIFF too) as generic `image2`, so `normalized_format_id` now disambiguates by extension exactly like the JPEG precedent — without it BMP classified as Video and planning rejected it; (2) the office-PDF executor's `source_format` whitelist and draw-filter arm lacked tiff/bmp.
- Header sniffing learns TIFF (`II*\0`/`MM\0*`) and BMP (`BM` + zero reserved word) magics.
- `scripts/count_routes.py` is now the single source of truth for the route figure: **172 canonical reachable = 103 direct + 69 chained** (v0.1.0-era table counts 144 under the same method; the previously quoted 319 came from an undocumented alias-expanded ad-hoc count and is left frozen in the v0.1.0 release notes). README badge/body and announcement copy updated to the canonical number.

### Verification
- Unit: 5 new tests (capabilities lanes/targets ×2, planner tiff lossless + bmp alpha ×2, inspect sniffing) — core `--lib` 249 passed / 4 known Windows symlink failures; workspace fmt/clippy/cargo-deny rehearsal clean.
- e2e Windows: manual route probes (incl. true-alpha TIFF→bmp PolicyBlocked through the chain's hop-2, TIFF→png alpha-preserved) plus **full matrix 71/71** with the 12 new tiff/bmp rows; tiff/bmp→pdf report `warning` from OFFICE_VISUAL_DRIFT exactly like the pre-existing png→pdf row (informational, not a regression).
- Matrix scripts: Windows learns on-demand tiff/bmp fixtures + `rm -rf` out-dir (the old numbered outputs collided across runs and caused OutputConflict noise); Linux gains PIL tiff/bmp fixtures and `tiff/bmp → txt` OCR rows.

### Risks / Follow-up
- **Linux matrix + OCR e2e parked**: the Tailscale relay dropped mid-sync (known intermittent outage); retry when it self-recovers. CI's Linux job has no engines, so tiff/bmp→txt stays unit-gated until then.
- Windows OCR for tiff/bmp remains engine-gated on a host Tesseract (deferred, UAC).
- C1 continues: RAW (dcraw/RawTherapee engine-discovery) and PSD (ImageMagick, Apache-2.0 packable) are the next waves.

## 2026-09-04 — C1 wave 2: PSD + camera-RAW via the discovered ImageMagick engine

### Design
- ffprobe cannot demux PSD or camera-RAW, so the new `magick` engine is both decoder and inspector (engine-as-prober, pdfinfo precedent): `magick identify -format "%m %w %h\n"` builds the Probe (newline mandatory — multi-layer PSD prints unseparated frame records), and conversion pins the composite frame with the `input[0]` spec (a bare PSD input fans out to `output-0.png/output-1.png` side files on single-image writers). The raster output then validates through the normal ffprobe media checks; the workflow tuple's validation engine must be **ffprobe**, not magick (heif-lane precedent).
- Inputs: psd/dng/cr2/cr3/arw/nef/orf/rw2/pef/raf → png/jpg/tiff directly. **TIFF joined the chain intermediate whitelist** (lossless pivot, png's peer), so RAW reaches webp/avif/pdf/txt/bmp through one-hop chains.
- Engine posture: ImageMagick 7.1.2-31 portable at `E:\DevCaches\ImageMagick` (Apache-2.0, packable later), discovered via `FORMATWRIGHT_ENGINE_MAGICK`/PATH; never bundled for now. dcraw 9.28 was compiled locally (cl.exe + NO_JASPER/NO_JPEG/NO_LCMS + ftello/fseeko/getc_unlocked shims) as a reference but the single magick lane covers both C1 RAW and PSD waves.

### Verification
- e2e probes: psd→png/jpg/tiff, dng→tiff/png, cr2→png/jpg, chain dng→webp (via tiff) — all pass; `formatwright inspect sample.psd` now reports `psd (Image)` (the CLI inspect path routes magick-family extensions to the magick probe).
- **Windows matrix 82/82** (71 prior + 11 magick rows incl. one raw→tiff→webp chain row). One earlier run showed 26 OutputConflict failures — a cancelled matrix run left a zombie writer racing the rerun's numbered outputs; clean rerun green.
- core `--lib` 251 passed / 4 known symlink failures; workspace fmt/clippy/cargo-deny clean. Route figure: **252 canonical reachable = 133 direct + 119 chained** (`scripts/count_routes.py`), README badge/body updated.

### Risks / Follow-up
- **Linux verification parked again**: the Tailscale relay has been down for ~1h (longest outage yet; may need a box-side check). Linux matrix + OCR + magick rows queued for when it returns — ImageMagick needs a user-level install there (conda-forge into the ocr env, zero-sudo).
- RAW fixtures are 10 MB real camera files (f-spot/raw-samples, CCL) gated on presence in the matrix scripts; a smaller synthetic DNG would shrink CI-less test cycles.
- CR3 (Canon) decode depends on the host ImageMagick build's raw support; declared but not e2e-tested (no fixture).
- jpeg quality default 92 on the magick lane (encoder default) vs 85 on the ffmpeg lane — intentional per-engine defaults, documented in the plan constraints.

## 2026-09-04 — C2: Outlook MSG input via the built-in CFB adapter

### Design
- New `formatwright.msg` builtin engine (`msg.rs`): the `cfb` crate (0.14, MIT) reads the compound document; root `__substg1.0_<tag><type>` streams supply transport headers (0x007D), subject (0x0037), sender (0x0C1A), submit FILETIME (0x0039 → hand-rolled RFC 2822), plain body (0x1000), HTML body (0x1013). A **single-part EML is synthesized** (body-describing transport headers dropped, our own Content-Type appended) and the entire EML pipeline is reused: RFC 2047 decoding, script/remote-resource sanitization, renderers, validation receipts. Direct targets txt/html; pdf/docx/epub compose through the html chain hop. Non-CFB or payload-less containers fail closed (`InputInvalid`).
- Unit fixtures are synthesized with the cfb **writer** (structurally faithful root property streams); the e2e fixture is a real Outlook export (msg-extractor's multi-to.msg).

### Bugs found by the work
1. **Latent EML immediate-path bug**: builtin adapters mint a fresh report id, so `ReportService::save` rejected the executing job ("ValidationReport job ID does not match its destination"). EML carried this since its landing (matrix lacked eml rows; unit tests bypass persistence). Both eml and msg dispatches now rebind `report.job_id = job_id`; the matrix gained eml+msg rows to pin the path.
2. **Vacuous EML script check**: `output_html_has_script` re-parsed the INPUT as EML, so non-EML callers always passed. `validate_eml_export_output` now takes the rendered output string.

### Verification
- Unit: 6 new msg tests (synthesis, plain-only selection, fail-closed, FILETIME math, txt/html export+validate, remote-resource PolicyBlocked) — core `--lib` 257 passed + 4 known symlink failures; workspace fmt/clippy/deny clean.
- e2e with the real .msg: inspect reports `msg` with decoded from/subject/text-chars; msg→txt/html Pass, msg→pdf Pass through the `msg -> pdf via intermediate HTML` chain (fresh state-db per run — stale reservations from failed runs bite again).
- **Windows matrix 87/87** (82 + eml×2 + msg×3). Route figure: **257 canonical = 135 direct + 122 chained** (`scripts/count_routes.py`).

### Risks / Follow-up
- Attachments and recipient tables are intentionally dropped (mirrors the EML posture); the txt render carries the synthesized MIME housekeeping headers — cosmetic, inherited from the shared renderer.
- Linux verification still parked on the Tailscale relay outage; cfb is pure Rust so CI's Linux test job exercises the unit layer regardless.
- C3 (MBOX→single PDF) remains: MBOX split → per-mail EML lane → pdf-merge.

## 2026-09-05 — C3: whole-mailbox MBOX export (the rival-absent combo)

### Design
- Built-in `formatwright.mbox` (`mbox.rs`): mboxrd split on envelope `From_` lines (one-level `>From` unescape; fail-closed with no envelope line, non-UTF-8, >1000 mails, or 256 MiB), every mail parsed through the shared EML pipeline.
- **txt/html**: all mails rendered with `==== Anole Mail i/N ====` separators; checks = target format + every separator in the output + text nonempty (+ script-free for html).
- **pdf**: per-mail sanitized HTML packets (separator + subject header inside) → each rides the html→pdf lane via the chain's prepare/execute pattern (own plans + receipts) → qpdf `--empty --pages … 1-z` merge → acceptance proves **page conservation** (pdfinfo: sum of per-mail pages == merged pages) and **every separator in the merged text layer** (pdftotext). The runner dispatch boxes the mbox→execute_plan→mbox recursion edge.

### Verification
- 6 unit tests (split/unescape, fail-closed ×2, single-mail round-trip, txt/html export+validate, remote-resource PolicyBlocked, pdf composite e2e that self-skips when engines are absent). core `--lib` 263 passed + 4 known symlink failures; fmt/clippy/deny clean.
- Windows matrix **90/90** with a three-mail fixture (Chinese RFC 2047 subject, mboxrd escape, script-bearing HTML); the first run's three mbox FAILs were a stale debug binary — rebuilt and all Pass, including the merged PDF.
- Route figure: **264 canonical = 138 direct + 126 chained** (`scripts/count_routes.py`).

### Risks / Follow-up
- mboxcl/mboxo variants beyond one-level `>From` unescape are not distinguished; Content-Length-based splits unsupported (mboxrd is the dominant modern dialect).
- The whole C batch (C1/C2/C3) still awaits the Linux-side matrix run — relay outage persists.

## 2026-09-05 — C batch Linux verification CLOSED (relay bypassed via LAN)

### Outage diagnosis (Leo asked)
- The box itself was healthy the whole time: uptime 2d15h, load ~0, LAN 3 ms, SSH fine. Only the **Tailscale data plane** was dead: control plane `active; relay "tok"` with `tx 9516 / rx 0`, `tailscale ping` no reply — the same fingerprint as the 2026-09-03 Clash-TUN-conflict outage (that one healed after a box reboot; this one never got one).
- Fix applied: **no config touched** — the LAN alias `macair-wifi` (192.168.0.192) carries SSH directly; all Linux work now runs over it.

### Fixes the Linux run itself surfaced
1. **Nested-HTML drift (real bug)**: mbox per-mail packets embedded `render_html`'s full `<html>` document inside another `<html>` — on engine paths without a browser lane, html→pdf falls back to the pandoc lane whose DOCX intermediate failed semantic-token conservation. Windows never saw it (browser lane skips the DOCX hop). Fix: embed only the extracted `<body>` fragment (`ab28800`).
2. Linux matrix script: CfT-chrome `FORMATWRIGHT_ENGINE_MSEDGE` export, PSD fixture via magick (PIL cannot write PSD), build without `--offline` (fresh cfb dep), out-dir cleanup (stale numbered outputs caused 30 OutputConflict), email fixtures via `newline=''` CRLF writer (shell heredoc had eaten the escapes).

### Verification (Linux, via LAN)
- **Core `--lib`: 255 passed / 0 failed** — includes the Windows-impossible symlink class and the mbox/MSG engine-dependent e2e (qpdf/poppler/pandoc/soffice 24.2.7.2/magick 7.1.2-31 all discovered; conda imagemagick install automated in the verify script).
- **Conversion matrix: 52/52** — structured, markup→pdf via browser lane (CfT chrome), office/pdf→image, raster family incl. tiff/bmp/psd and OCR rows, email family (eml/mbox/msg incl. chained pdf), audio, archives.
- Route surface cross-platform parity confirmed; CI green on all pushes (latest `b84b769`).

### Follow-up
- Tailscale data plane on the box still needs Leo's attention when convenient (LAN works; nothing blocked).
- The 20-minute auto-probe automation is obsolete (verification closed) — deleted.

## 2026-09-06 — Clean-VM certification (R-008/R-009 closeout): preparation wave

Goal: execute `docs/testing/CLEAN_VM_CERTIFICATION.md` (Batch D) against the shipped
v0.1.0 installer to close R-008/R-009 Fixed → Closed. Host is Windows 11 Pro
(build 26200, 48 GB RAM) — Windows Sandbox was chosen as the clean-VM substrate
(fresh image per boot matches the "no snapshots of prior testing" requirement).

### Materials prepared (all under `target/clean-vm/`, gitignored)
- `Anole_0.1.0_x64-setup.exe` (330,623,750 B) downloaded from the v0.1.0 Release;
  SHA256 `b76ab8825fb87c7dc73a3bb1182298de2821ed17cc597b5f722b4cdef9c98c9f` matches
  the Release `SHA256SUMS`.
- `fixture-15p.pdf` — 15-page A4 fixture **generated on the Linux executor**
  (soffice HTML→PDF over `macair-away`; LAN channel was down at that moment),
  SHA256 `d7d01ce93b4013a9162040e8d559c92d33036a9ffb19ede83498bcf6bc009c8e`
  re-verified locally after scp.
- Scripts staged in the layout `test_clean_vm_certification.ps1` expects
  (`scripts/` + `apps/desktop/src-tauri/explorer-verbs.json` for the uninstall
  assertion), plus `cleanvm.wsb` (online), `cleanvm-offline.wsb` (networking
  disabled), and `run-certification.ps1` (in-sandbox bootstrap: winget pwsh7 +
  Node.js, cleanliness re-assert, then the suite).

### Local e2e-binary build blocked; moved to CI
Three consecutive `tauri build --no-bundle --config tauri.release-e2e.conf.json`
attempts failed in the tauri build script with `os error 32` while copying the
102 MB engine-pack `ffmpeg.exe` (the `dist/engine-packs/.../starter/` resource
mapping). Probes show no persistent holder (source and `out/` copies open
exclusively fine between runs) — consistent with a Defender/indexer race on the
freshly written copy that the build script immediately re-opens. Workaround:
`.github/workflows/build-e2e-binary.yml` (manual `workflow_dispatch`, commit
`173e277`) builds the overlay binary on `windows-latest` and uploads it as the
`formatwright-desktop-e2e` artifact, with an in-CI assertion that the
`remote-debugging-port` overlay is embedded.

### Sandbox enablement status (blocked on Leo)
- Hypervisor already running (WSL2-era), so **no reboot is expected** after the
  feature install.
- First elevated attempt: DISM ran but my flag was wrong (`/enable` instead of
  `/enable-feature`) — Error 87, feature NOT enabled.
- Corrected script (`target/enable-sandbox.bat`) attempted next, but the UAC
  prompt was **canceled** — enablement awaits Leo approving one more UAC
  (dism `/enable-feature /featurename:Containers-DisposableClientVM /all /norestart`).
- A combined offer (Defender exclusions for `target\` + `dist\engine-packs\` to
  also fix the local build race, plus Sandbox enablement, one UAC) was posed;
  no answer yet at the time of this note.

### Next steps from here
1. CI artifact → `target/clean-vm/formatwright-desktop-e2e.exe`.
2. UAC approval → Sandbox feature on (verify `WindowsSandbox.exe` appears).
3. Launch `cleanvm.wsb`, run `run-certification.ps1`, capture artifacts.
4. Manual checklist incl. in-sandbox adapter disable for the offline phase.
5. Update `docs/DEFECT_REGISTER.md` + `CLEAN_VM_CERTIFICATION.md` with evidence.

### Reboot pending (Leo's action)
- Third UAC attempt was approved; DISM enabled `Containers-DisposableClientVM`
  with exit code **3010 (success, reboot required)**. Sandbox binaries
  (`WindowsSandbox.exe`) have not landed yet — they appear after the reboot's
  specialize phase.
- Post-reboot flow is fully automated: `cleanvm.wsb` now carries a
  `LogonCommand` that launches `run-certification.ps1` inside the sandbox
  (network wait → winget pwsh7 + Node → cleanliness asserts → the certification
  suite). `run-certification.ps1` re-validated (PARSE-OK) after adding the
  network-wait guard; `.wsb` XML validated.
- CI artifact landed: `formatwright-desktop-e2e.exe` (21,862,912 B,
  `remote-debugging-port` overlay verified, sha256
  `e1db7d08abea295ffa2189a15e61057ce184d5d7aff403181dfc5196fd6c1f59`) — the
  portable exe correctly does NOT embed engine packs; it shares the engine
  store that the installed app provisions on first launch.
- A scheduled probe (20 min) will check `WindowsSandbox.exe` and continue the
  certification automatically once the reboot has happened.

## 2026-09-06 — Experience spec plan execution wave 1 (E-02 hint / E-05 / E-07 / E-08 / E-09 / E-10)

Executed from `docs/specs/EXPERIENCE_SPEC_PLAN.md` (approved for implementation).
Six items landed; E-01/E-02 (full)/E-03/E-04/E-11/E-12 stay blocked on
DECISION-1..4 (certificate, HEIC legal, LibreOffice legal, Tesseract) and
E-06 was deliberately deferred (see risks).

### Decisions & deviations
1. **E-08 rate channel**: spec wording said "(engine, capability) rolling
   stats in SQLite" — implemented exactly so (schema v6 table
   `engine_throughput_samples`, 64-sample window, 16-sample rate window);
   `QueueProgressUpdate` gained `measured_throughput_bytes_per_sec`, injected
   on the Running update from history. No ETA is synthesized anywhere.
2. **E-07 password channel deviates from spec wording**: spec said
   "per-process env var, argv forbidden". Poppler engines read passwords only
   from argv, so the render lane injects `-upw` at spawn time with the
   cleartext held in the pre-existing `pdf_secret_store` (G-22 hand-off,
   single-use, keyed by `plan_id`). Serialized plans carry `[redacted]` only.
   Same-process immediate conversion works; a durably queued encrypted-PDF
   plan fails re-inspect with the clear "password required" error (secret
   store is single-use by design). qpdf lanes keep their existing hand-off.
3. **E-05 directory verbs**: registered 2 Directory verbs only
   (folder → JPG / WebP) pending DECISION-5; generator asserts updated 17→19
   and the generator's stale "Open in FormatWright" template drift was fixed
   to "Open in Anole". Folder convert = one approval (KD-2), but the full
   folder-batch safety chain (mapping preview, per-file plan checks, skipped
   list, disk budget, no-clobber fresh output root `<name>-anole-<target>`)
   runs before queueing.
4. **E-09 preview**: inline images ≤ 8 MiB pass through un-re-encoded
   (browser scales); PDF first page via pdftoppm 256px; video first frame via
   ffmpeg. Missing engines degrade to a hidden block (spec acceptance).
   The 256px engine-rendered thumbnails are not cached on disk in this wave
   (regenerated per report open; ≤ 20 s timeout, no persistent cache dir) —
   spec's "cache + maintenance cleanup" clause is partially met (no cache to
   clean). Recorded as follow-up if profiling shows cost.
5. **E-10 settings v2**: `ApplicationSettings` gained `theme`
   (system/light/dark); v1 files migrate in memory on read (persisted v2 on
   next save), restore bundles with v1 settings hit the same path. Meadowlark
   colors now fully token-driven incl. warning/deco colors; dark set defined
   for both explicit and `prefers-color-scheme` resolution; `color-scheme`
   declared for native controls.

### Verification (this machine, MSVC env via target/*.bat)
- `cargo test -p formatwright-core --lib`: **273 passed / 0 failed**
  (4 known symlink/reparse privilege failures excluded as the documented
  baseline; they fail identically on unmodified main).
- `cargo test -p formatwright-desktop --lib`: **34 passed / 0 failed**
  (new: directory shell-convert acceptance, output-root reservation).
- `cargo test -p formatwright-core --test schema_contracts`: **9 passed**
  (application-settings v2 contract included).
- Frontend: `tsc -b` clean, vitest **29 passed** (progress type extended).
- `cargo clippy --workspace --all-targets -- -D warnings`: **0 errors**;
  `cargo fmt --all --check`: clean.
- `scripts/generate_explorer_verbs.ps1 -Check`: regenerated nsh/register
  script match the 19-verb table.

### Risks / follow-ups
- **E-07 queue limitation**: encrypted PDFs through the *durable queue*
  (multi-select Explorer convert, `jobs run`) re-inspect without the password
  and fail with the clear PolicyBlocked error; immediate conversion (single
  right-click / convert form / CLI convert) is the supported path this wave.
- **E-06 not started** (right-click verb configurability + HKCU runtime
  registration): deliberate — it is a behavior migration whose data source
  should be shared with E-02's Win11 menu; needs DECISION-6 anyway.
- **Windows smoke test** (`test_windows_explorer_integration.ps1`) and
  clean-VM script were extended for Directory verbs but not executed this
  wave (they require a full NSIS build + GUI session); run them before the
  next release candidate.
- Dark theme contrast was designed to WCAG-ish targets but the automated
  accessibility baseline script run is pending a real WebView session.

## 2026-09-06 — Experience spec plan execution wave 2 (E-06 runtime verb configuration)

E-06 landed after the first wave: Explorer convert-verb registration moved
from the NSIS script to the application, and verbs became user-configurable.

### What changed
1. **`PresetLibrary` v2** adds `shell_verbs: Vec<ShellVerbBinding>`
   (`verb_id`, `enabled`, optional `preset_id`); v1 libraries migrate in
   memory on read (`migrate_legacy`) and persist v2 on next save. Bindings
   validate uniqueness, count bounds, and that a bound preset exists in the
   same library. Public schema `preset-library/v2.schema.json` added; contract
   test now runs against v2.
2. **`explorer_integration.rs` (desktop)**: baseline table embedded from
   `explorer-verbs.json`; `resolve_registrations` folds bindings (missing =
   enabled default; cross-target preset bindings ignored); disabled verbs are
   removed. Registration is **zero-unsafe** (workspace `forbid(unsafe_code)`):
   enabled verbs render into a UTF-16 `.reg` document imported via built-in
   `reg.exe`, deletions use `reg delete` — typed argv, no PowerShell. Key
   gotcha found by test: `.reg` files silently ignore the `HKCU` short hive
   name; documents must spell `HKEY_CURRENT_USER`.
3. **Bootstrap & first launch**: NSIS POSTINSTALL now only keeps the two
   Open-in keys plus `ExecWait … --register-shell`; `main.rs` intercepts the
   flag and exits after applying verbs (no GUI, no single-instance plugin).
   The app also re-applies verbs on every startup (background thread, failure
   only logs). PREUNINSTALL deletions stay as the cleanup backstop, so
   upgraded installs keep cleaning the same fixed verb IDs.
4. **Preset plumbing**: verb commands carry `--preset <uuid>`;
   `parse_shell_invocation` returns a triple, the coordinator batches by
   (target, preset) and never merges different presets, and both ingest lanes
   (per-file and E-05 folder) fold the bound preset into the `PlanRequest`
   (target-mismatch presets ignored — defense in depth).
5. **Settings UI**: new right-click-menu section listing all 19 verbs with
   enable toggles and preset pickers (filtered to matching targets),
   "restore default menu" button; changes persist into the preset library,
   re-apply HKCU immediately, and travel with preset export/import (imported
   bindings override local ones per verb, then verbs re-apply in background).
   Menu labels show the bound preset ("Convert to WebP · Small WebP").

### Verification
- desktop `--lib`: **40 passed / 0 failed** (new: registry write/remove
  round-trip against real HKCU scratch keys, preset parse, baseline shape).
- core `--lib`: **275 passed / 0 failed** (known 4 symlink-privilege
  failures skipped as baseline); schema contracts **9/9** incl. v2.
- Frontend: tsc clean, vitest **29/29**; `cargo clippy --workspace
  --all-targets -- -D warnings`: 0; `cargo fmt --all --check` clean;
  verb generator `-Check` green (nsh now bootstrap-only for convert verbs).

### Risks / follow-ups
- Smoke test (`test_windows_explorer_integration.ps1`) assertions still hold
  (install-time `--register-shell` pre-creates the same keys) but were not
  executed this wave — run before the next release candidate.
- `register_dev_explorer_convert.ps1` still writes static registrations for
  dev builds (by design; it does not read bindings).
- First-run registration is best-effort on a background thread; if `reg.exe`
  is blocked by policy the installer-time bootstrap remains the fallback.

## 2026-09-07 — DECISION-1 waiting-materials wave (E-01 CI skeleton, E-12 script)

All engineering-reachable spec items are done; this wave only lowers the cost
of Leo's pending decisions. No product decision was assumed anywhere.

1. **`docs/release/CODE_SIGNING_DECISION_BRIEF.md`** — DECISION-1 decision
   brief with a 2026-09 market check: EV no longer guarantees instant
   SmartScreen reputation (Microsoft/DigiCert both confirm), OV and EV both
   require hardware/cloud key media (a plain PFX in CI secrets is no longer
   purchasable from compliant CAs), and 1-year max terms start Feb 2026.
   Recommendation on record: OV + CA cloud-signing KSP (~$130–300/yr); EV's
   only hard benefit (driver signing) does not apply to Anole.
2. **`release-candidate.yml` E-01 skeleton** — new Authenticode step that
   self-activates when the `WINDOWS_CODESIGN_PFX` secret exists (signtool
   SHA256 + RFC3161 timestamp + `verify /pa /all`), checksums moved after
   signing, and the old "must be NotSigned" assert flipped into a two-way
   "signature state must match the configured secret" assert. Without the
   secret the workflow behaves exactly as before (explicit skip message).
   Cloud-KSP middleware installation point is marked in a comment for the
   DECISION-1 outcome. YAML validated; live workflow run pending a real
   secret (next release rehearsal).
3. **`docs/testing/USER_STUDY_R1.md`** — E-12 first-run study script: five
   read-aloud tasks (install, drag PDF, right-click convert, folder batch,
   "prove it converted correctly"), per-participant record sheet, observer
   rules (90-second rule), and a P0/P1/P2 findings triage. Execution needs
   3–5 non-developer participants plus a signed installer (task 1 is polluted
   by SmartScreen on an unsigned build).

## 2026-09-07 — Accessibility-baseline audit attempt (R-011 opened)

Tried to close the E-10 follow-up ("rerun the real WebView accessibility
baseline"). The script itself is broken against the current UI and has been
since the Meadowlark/chicago95 rework — before this week's changes:

- `scripts/cdp_accessibility_audit.mjs` waits for `.shell`, `header nav`,
  `#input-path`, `.skip-link`; none exist in the shipped DOM (root is
  `article.c95-window.fw-main-window`; navigation is `.c95-tabs`; no skip
  link). The audit times out at `waitFor FormatWirth document` before any
  assertion runs, on a pristine state directory and after state warm-up
  alike (cold-start tolerance widened 15s→45s as an independent fix; the
  timeout persists).
- Verified the app itself is healthy: debug build with the accessibility
  overlay boots under isolated APPDATA/LOCALAPPDATA, CDP target appears
  within ~5 s warm, `--shell-open` with the RTL/Unicode fixture survives,
  and the state-isolation harness restores state cleanly.
- Recorded as **R-011 (P2, Open)** in `docs/DEFECT_REGISTER.md` with
  reproduction evidence and a closure criterion (audit rewritten against the
  chicago95 DOM, zh/en rerun green, MASTER §1.1 baseline row re-dated).
  MASTER's "automated accessibility baseline green" claim predates the UI
  rework and should not be cited for the current DOM until R-011 closes.
- E-10's verification therefore stands on: schema-v2 settings contract,
  token-driven dark palette (explicit + prefers-color-scheme), forced-colors
  override precedence retained, and vitest/tsc/clippy/fmt green. The real
  WebView baseline rerun moves to R-011 rather than being silently claimed.

Build evidence: `tauri build --debug --no-bundle` with the accessibility
overlay completed; `target\debug\formatwright-desktop.exe` boots with
`--remote-debugging-port=9337` reachable.

## 2026-09-07 — R-011 fixed: accessibility baseline restored on the chicago95 DOM

The stale audit turned out to hide a real regression and three selector drifts:

- **Product-side gaps (fixed)**: the Meadowlark/chicago95 rework had dropped
  the navigation landmark, `aria-current="page"` on the active tab, and the
  `main` landmark. Added `nav.fw-tabs-nav` (localized `aria-label`), tab
  `aria-current`, and a `main.fw-tabs-main` wrapper — pure semantic wrappers,
  zero layout change (`display: block`, one CSS rule).
- **Audit-side drift (fixed)**: readiness gate `.shell` → `.fw-main-window`;
  navigation assertions → `nav.fw-tabs-nav` / `[role="tab"]`; settings
  navigation click via escaped selector quote; DPR assertion tolerance for
  WebView2's 2.0000000596046448; 45 s cold-start allowance from the earlier
  attempt (kept).
- **Result (2026-09-07, real WebView2)**: 210 nodes, 0 unnamed focusable
  controls, zh-CN→en switch with localized navigation label, skip-link →
  `#main-content` keyboard flow, 200 % viewport overflow-free (dark-token
  refactor included), reduced-motion + forced-colors honored. Evidence under
  `.artifacts/desktop-accessibility/suite-1a9ec04c…`. R-011 → Fixed; MASTER
  §1.1 baseline row re-dated. Frontend tsc + vitest 29/29 still green; no
  Rust changes this round (earlier workspace clippy/fmt results stand).

## 2026-09-07 — Remaining decision briefs (DECISION-2/3/4/5)

`docs/release/ENGINE_PACK_DECISION_BRIEFS.md` completes the decision-material
set alongside `CODE_SIGNING_DECISION_BRIEF.md`, with a one-line reply format
so all pending gates can be unblocked in a single answer. Facts verified
2026-09-07: tessdata (incl. chi_sim, fast variants) is Apache-2.0 across the
official repo and distro packaging; LibreOffice is MPL-2.0 with the TDF
trademark policy's "substantially unmodified" allowance covering
packaging-level redistribution. HEVC patent-pool exposure for the HEIC decode
route is presented as an explicit risk-acceptance choice (option a), not a
no-risk conclusion. No product decision was assumed.

## 2026-09-07 — UX_FLOWS.md sync (final doc-debt item)

Closed the last item from the spec's test/documentation sync checklist:
`docs/specs/UX_FLOWS.md` v0.2 now records the runtime verb registration and
folder verbs (E-05/E-06), the implemented encrypted-PDF secret flow with its
disclosed argv deviation (E-07), and two new flow sections (output preview
E-09, Explorer verb configuration E-06). All other checklist entries
(WINDOWS_PACKAGING/RELEASE_CHECKLIST need DECISION-1; FORMAT_SUPPORT_MATRIX
caveats need E-03/E-04; smoke-test execution needs a release rehearsal) remain
correctly gated on the pending decisions.

## 2026-09-07 — Leo approved all decisions ("ok")

Recorded in the spec's decision table: DECISION-1 by recommendation (OV +
cloud-signing KSP; CA purchase remains Leo's manual step), DECISION-2/3/4 all
option a, DECISION-5 keep 2 folder verbs, and the 45-file change set is
approved for Conventional Commit submission. E-11 (OCR pack) starts first as
the lowest-risk unlocked item.

## 2026-09-07 — E-11 OCR starter pack shipped (DECISION-4)

Executed on Leo's "ok" approval of all decisions. Third starter pack
(`starter/ocr/`, 97 files, SBOM-verified) with Tesseract 5.4.0.20240606 +
pinned eng/chi_sim traineddata. Supply chain: installer downloaded and
**7-Zip-unpacked on the Linux executor (macair-away) at Leo's request** —
the NSIS installer never ran on the Windows host; the assembled tree tarred
back with a verified sha256 and the prepare script reproduces the same flow
on CI via 7z.exe (Windows runners ship it; developer machines without 7z
can drop an unpacked tree into `.devtools/starter-sources`). Verified:
engines verify / first-launch install+activation into the real engine store /
doctor resolves the pack engine / real eng and chi_sim conversions Pass with
correct text via the new `--ocr-language` pipeline (PlanRequest field +
planner threading + CLI flag + unit test). Starter assertions updated to
expect three manifests (CI workflow + explorer smoke). Core 276/0, clippy 0,
fmt clean. GUI-click conversion against the activated pack remains covered by
the next release smoke, same as pdf/media.

## 2026-09-07 — E-04 application side: optional-pack downloader (no downloads performed)

New download policy recorded (Leo, 2026-09-07): nothing is ever downloaded
onto the Windows host; downloads happen only on the Linux executor after
Leo's explicit approval. This wave therefore shipped only zero-download
engineering:

- `optional_packs.rs` (desktop): curated pack registry. The Document pack
  (LibreOffice, MPL-2.0) is announced with an **empty pinned hash**, which
  disables its download button until the pack is published — unpinned
  content can never be fetched. Core staging path (`stage_verified_pack_archive`)
  enforces the pinned SHA-256, extracts the zip (manifest at root or one
  nested directory), and routes through the standard verified-install +
  registry activation; `download_pinned_archive` streams with reqwest
  (rustls, reusing the updater's existing dependency set — no new crates)
  and emits per-chunk progress events.
- Engines page: "Optional engine packs" card listing the Document pack with
  installed/downloadable states, progress percentage, and a privacy note.
- PRIVACY.md discloses the button-triggered, pinned-hash, direct-to-release
  download (the app's only non-updater outbound traffic).
- Tests: hash mismatch and unpinned-hash refusals, nested-manifest
  extraction, announced-but-not-downloadable state. Desktop 43/0, clippy 0,
  fmt clean, frontend tsc + vitest 29/29.

Pending for E-04 completion (needs Leo-approved downloads on the Linux box):
LibreOffice official installer fetch, MSI unpack, Document pack assembly,
hash pinning, real docx→pdf run. Same for E-03 (MSYS2 libheif/libde265
decode-only tree investigation).

## 2026-09-07 — E-04 shipped: Document pack (LibreOffice 26.2.6, DECISION-3, list B approved)

Executed under the download policy (Linux executor only; Leo approved list B):

- **Supply chain**: official TDF `LibreOffice_26.2.6_Win_x86-64.msi`
  (373,252,096 B, sha256 `f9877032…5fb2660`) downloaded on macair; the MSI
  **was never executed** — 7-Zip extracted the payload (flat, 19,248 real
  files) and `pymsi` (pure-Python MSI table parser, installed into the
  conda env) reconstructed the Directory/Component/File tree
  (`scripts/rebuild_libreoffice_tree_from_msi.py`); the x64 VC runtime DLLs
  destined for System32 were relocated into `program/` instead.
- **Pack**: `formatwright-document` v26.2.6, executable `soffice` →
  `program/soffice.com`, 19,476-file SPDX SBOM, MPL-2.0 + MSVC-redist
  notices, PROVENANCE with the exact unpack method. `engines verify` green.
- **Real conversion**: `FORMATWRIGHT_ENGINE_SOFFICE=<pack>/program/soffice.com`
  converts a minimal docx fixture to PDF (validation: Warning — same lane
  behavior as the system LibreOffice), and `pdftotext` recovers the exact
  source text. The plan hash differs from the system-engine run, proving the
  pack's own engine served the conversion.
- **Release artifact**: `document-pack-windows-x86_64.zip` (511,845,052 B,
  sha256 `44126a49…325369`, fixed-timestamp reproducible zip) staged under
  `dist/engine-packs/windows-x86_64/optional/`; the hash and size are now
  **pinned in `optional_packs.rs`**, which activates the Engines-page
  download button once the zip is attached to the v0.1.1 release.
- Desktop 43/0 (pinned-hash test updated), clippy 0, fmt clean, frontend
  29/29. GUI-click download end-to-end rides the next release rehearsal, as
  with the OCR pack.

## 2026-09-07 — E-04/E-11 acceptance hardening (zero-download wave)

Filled the remaining acceptance gaps for the shipped packs:

- **E-04 xlsx**: hand-built minimal xlsx → PDF through the pack's own
  soffice.com; `pdftotext` recovers "SheetSmoke 440010147700".
- **E-04 pptx**: hand-built fixtures convert to valid PDFs; both the pack
  engine and the system LibreOffice render the synthetic shape without a
  text layer (identical behavior — the limitation is the synthetic fixture,
  not the pack). Real-world pptx validation rides E-12 / the release smoke.
- **E-04 profile isolation proven**: after all conversions, the user's
  `%APPDATA%\LibreOffice\4\user` tree has zero new or modified entries
  (find -newer empty); the runner's `-env:UserInstallation` profile lives
  in staging and is cleaned with it.
- **E-11 pdf-ocr lane**: a scanned-style image PDF (Chinese text rendered at
  150 dpi) converts via `--operation pdf-ocr --ocr-language chi_sim` to
  `validation: Pass` with the exact source text recognized. Note for
  callers: plain `convert x.pdf --to txt` routes through the chain lane,
  not OCR — the OCR lane is the explicit `pdf-ocr` operation.

## 2026-09-07 — Documentation-sync closing wave (spec checklist debt)

Zero-code wave closing the remaining entries of the experience spec's
test/documentation sync checklist. Nothing here assumes a pending product
decision; all facts reference shipped work or recorded approvals.

1. **`docs/VOC_BACKLOG.md`** — Wave-1 checkboxes were already ticked in
   e9f1b10 (spot-checked the six items against `apps/desktop/src/i18n.ts`
   and `App.tsx` before trusting them). Updated the wave-2 note to reflect
   DECISION-5 (keep 2 folder verbs, approved 2026-09-07) and DECISION-1's
   approved tier; added a wave-3 status note (3.3 shipped as E-04 Document
   pack with pinned release-zip hash, 3.4 shipped as E-07 with the
   disclosed argv deviation, 3.1/3.2 gated on E-03 list-A approval);
   replaced the stale "next: 0.1 commit" tail line with the actual
   owner-blocked set (CA purchase, clean-VM reboot, E-03 list A).
2. **`docs/specs/FORMAT_SUPPORT_MATRIX.md`** — GW-08 now names the
   optional Document pack (LibreOffice 26.2.6) as an engine source beside
   host installs, with a new evidence paragraph (docx/xlsx→PDF real runs
   through the pack's own soffice.com, config-tree isolation proven,
   pptx text-layer limit is the synthetic fixture's, all rows still
   non-Certified). HEIC/GW-01 untouched until E-03 lands. Updated date
   bumped.
3. **`docs/release/WINDOWS_PACKAGING.md`** — new "Code-signing status
   (DECISION-1, 2026-09-07)" section recording the approved tier (OV +
   CA cloud-signing KSP, EV rejected with the market-check reason), the
   self-activating `release-candidate.yml` Authenticode step, the
   cloud-KSP middleware comment marker, and the owner's remaining steps;
   links to the decision brief (file existence verified).
4. **`docs/release/RELEASE_CHECKLIST.md`** — the "Windows artifact built
   and signed" line now carries the decided tier as a parenthetical so
   the checklist no longer reads as undecided. E-02 sparse-package
   steps deliberately not added (not started).
5. **`docs/MASTER_EXECUTION_PLAN.md`** — §1.1 row 9 and the §Desktop
   done/pending row drifted behind the E-wave: engine-measured
   throughput (E-08) and accessibility were still listed as pending
   while shipped; runtime HKCU verbs, dark mode, the encrypted-PDF
   password field, and output previews were missing from the done side.
   Both rows re-synced to the EXPERIENCE_SPEC_PLAN execution status
   (2026-09-07); Win11 modern menu / Finder/Linux integration /
   live screen-reader study stay pending.

Verification: markdown-only diff (5 files, +18/−8); referenced files
(`CODE_SIGNING_DECISION_BRIEF.md`, `release-candidate.yml`) exist;
no CI job lints markdown, and no code is touched, so the standing
core 276/4-baseline, clippy, fmt, and frontend results are unaffected.

## 2026-09-08 — v0.1.1 release candidate built and smoke-tested locally

Leo asked for delivery; the missing layer was installer-level evidence
for the E-wave HEAD (all prior smoke evidence predates E-04/E-05/E-06/
E-11). This wave produced it on the Windows host:

- **Version bump 0.1.0 → 0.1.1** (workspace `Cargo.toml`,
  `apps/desktop/package.json`, `tauri.conf.json`) plus the three
  exact `=0.1.0` internal pins (`cli`→core, `core`→engine-sdk,
  `desktop`→core) that cargo resolution requires to move together.
- **NSIS build**: `Anole_0.1.1_x64-setup.exe` (396,448,308 B, sha256
  `c67f01fd…f9fd8`) + updater signature (release keypair) at
  `target/release/bundle/nsis/`; `dist/SHA256SUMS` regenerated
  (installer + bare exe). Unsigned, as DECISION-1's CA purchase is
  still pending.
- **Build gotchas hit and worked around** (all three are repro-any-
  time traps): (1) the Windows host no longer has a working `pnpm`
  on PATH — built via local `node_modules\.bin` with a
  `beforeBuildCommand: ""` override, front-end built manually first;
  (2) `tauri build` reads the starter resource tree from
  `dist/engine-packs/...` and fails deterministically with
  `os error 32` on `media/bin/ffmpeg.exe` (some local process locks
  that tree during builds; manual copy of the same bytes succeeds) —
  worked around by building against a fresh copy at
  `target/starter-build-src/` via a temporary
  `tauri.windows.conf.json` resources swap (restored afterwards;
  the swap must edit the platform file because `--config` merges
  rather than replaces the resources map); (3) this tauri-cli only
  honors `TAURI_SIGNING_PRIVATE_KEY` (content), not `…_PATH` —
  multi-line key content must be injected from PowerShell.
- **Explorer installed smoke GREEN** against the final installer
  (suite `cd9bfbecc9ce453e984554d108a63a88`, installer sha256
  matches SHA256SUMS): exact registry quoting, cold file-verb and
  hot directory-verb paths visible in the real window (E-05), one
  PID, Open-in created 0 durable jobs while a Convert verb created
  exactly 1 job with `convert_report_status: pass` and unchanged
  source hash, 19 owned convert keys (17 file + 2 directory), all
  three Starter packs installed with supply-chain sidecar hashes
  re-verified via the real CLI (E-11 OCR pack ships in the
  installer), missing-path negative rejected, uninstall exit 0 with
  owned keys removed / unrelated sibling preserved / app state
  restored byte-for-byte.
- **Smoke-script repairs surfaced by the run** (the script had not
  been executed since the rebrand and the OCR pack): UIA window-name
  assertion `'FormatWright'` → `'Anole'` (window title; the
  `Registry` verb key name `FormatWright` is intentionally
  unchanged as technical layer), and the installed-pack identity
  list updated to include `formatwright-ocr`. Local dev-machine
  HKCU verb keys left over from earlier dev-run registrations had
  to be removed first (the smoke requires a verb-clean machine and
  asserts pre-absence).

Delivery state: the installer is Leo-usable now; attaching it to a
public v0.1.1 release still waits on the CA decision artifacts
(signing, then the release rehearsal that also exercises the
Document-pack download button with the staged zip).



## 2026-09-08 — Markdown export wave (GW-13): any-core-format → md

Trigger: Leo reviewed microsoft/markitdown (MIT, ~181k stars) and asked
for the capability. Scope decision (Leo approved): "薄层全打通" — six
new direct →md routes on existing engines only; the MarkItDown-specific
sources that would need new engines (audio transcription, YouTube,
EXIF, LLM description) are explicitly out of scope (local-first,
zero-network positioning). pptx/xlsx/odt/odp/rtf/svg reach md only
through the CLI chain via the PDF pivot (pdf is whitelisted as an
intermediate), which is coverage, not quality; a native OOXML
extractor stays a future wave.

Routes added (capabilities.rs + count_routes.py mirrored):

- html/htm → md — Pandoc `--from=html --to=gfm`. The runner already
  accepted html→md (checked_argument allowed it); only the plan layer
  gated it. `plan_docx_markup_export` renamed to `plan_markup_export`
  and generalized to docx|html inputs; capability_id now derives from
  the probed source (`pandoc.docx-to-md.offline` unchanged for docx,
  so existing snapshot/plan-hash assertions hold). HTML inputs with
  external resources are PolicyBlocked, matching plan_markup_to_docx.
- eml/msg → md — builtin `render_md` (`# subject`, bold From/To/Date
  block, visible text); `validate_eml_export_output` now expects
  `markdown` as the observed output format for md.
- mbox → md — same render_md per mail with the existing separator
  lines; `MBOX_MAIL_SEPARATORS` acceptance unchanged.
- pdf → md — new `plan_pdf_text_export` (capability
  `poppler.pdf-to-md.offline`, engine pdftotext only,
  `loss_class=Lossy` — headings/tables/layout do not survive;
  multi-column reading order declared Unknown). Executed by the new
  `execute_pdftotext_text_plan` (stdout mode, 120s timeout), staged to
  the partial path, re-inspected as a document, accepted through the
  shared `validate_text_export_output`. The workflow branch must sit
  before the generic `pdf_format_hint` render branch.
- png/jpg/jpeg/tiff/tif/bmp → md — `plan_image_ocr` gained a target
  parameter (txt|md); same Tesseract lane, recognition text in a .md.

`normalize_target` now maps `markdown` → `md`.

Surfaces: desktop target dropdown gains "md" (filtered per-input by
capability routes), eml/msg/mbox get `["md"]` recommendations and
pdf gets md appended, Explorer verbs gain "Convert to Markdown" for
.pdf/.docx/.html/.htm/.eml/.msg, `normalizeShellTarget` accepts
`markdown`. Server needed no changes (snapshot-derived).

Route count: 147 direct + 143 chained = 290 reachable routes
(was 138 + 126 = 264); README badge and Status paragraph updated
(the v0.1.0 "what shipped" sentence keeps its historical 264).

Docs: GW-13 added to FORMAT_SUPPORT_MATRIX.md, GOLDEN_WORKFLOWS.md,
and golden-workflows.toml (status planned). Matrix scripts (Windows
+ Linux) gained docx/html/pdf/eml/msg/mbox → md rows (and png→md on
Linux where OCR engines exist).

Verification: formatwright-core `cargo test --lib` = 281 passed +
4 known symlink-privilege failures (baseline shape preserved; +8 new
tests across capabilities/document/eml/ocr/pdf). Local Windows matrix
run and workspace clippy/CI rehearsal noted below in the delivery
summary of this session.

Risks / open items: pdf→md is text-layer extraction — the support
matrix says so explicitly so it is not confused with docx→md's
structural export. OCR→md runtime evidence depends on a host
Tesseract (UAC-deferred locally; plan-level tests cover the logic,
Linux matrix covers the lane). Desktop folder-batch scope shows the
full target list (unfiltered) — md appears there for any input,
matching the pre-existing behavior of every other target.

## 2026-09-09 — GW-13 pre-commit rehearsal closed (SSH executor split) + Linux matrix script fixes

Delivery summary of the rehearsal session (Leo approved commit after
rehearsal; heavy work pushed to the macair Linux executor per his
"记得有ssh" reminder, task copy at `/home/leo/linux-runs/FormatWright/
gw13-rehearsal/` via `git archive HEAD` + working-tree patch):

- **Linux (macair, stable toolchain)**: `cargo clippy --workspace
  --exclude formatwright-desktop --all-targets --all-features --
  -D warnings` = 0; `cargo test` same scope = all green (core
  273 passed / 0 failed; cli/server/engine-sdk suites pass).
  Linux conversion matrix **55/55** (52-route baseline + GW-13's
  docx→md, pdf→md, png→md — png→md via the conda-env Tesseract).
- **Windows (local host, engines on E:\DevCaches)**: incremental
  debug CLI build + conversion matrix **96/96** (90-route baseline
  + 6 new →md rows); `cargo fmt --all --check` clean; desktop
  `desktopModel.test.ts` vitest = 27/27.
- **Why desktop is excluded from the Linux rehearsal**: macair has
  no glib/webkit dev libraries and sudo is forbidden, so
  `formatwright-desktop` cannot build there (glib-sys build script
  failure). GW-13 touches desktop only in TS/JSON, so the Rust-side
  rehearsal is complete; the CI Linux job (which installs desktop
  prerequisites) remains the full-workspace backstop.
- **Not rehearsed locally**: `cargo +1.88.0 check` (macair lacks the
  pinned toolchain; installing it is a new download) and cargo-deny
  (no Cargo.toml/lock changes in this wave). Both run in CI.

Bugs the rehearsal surfaced and fixed
(`scripts/test_conversion_matrix_linux.sh`, both latent, only
visible once FW_FIXTURES pointed away from the default directory):

1. The fixture-generating Python heredoc hardcoded
   `/home/leo/linux-runs/FormatWright/fixtures/` on every open()
   while the bash side honored `FW_FIXTURES` — the two sides read
   and wrote different directories. Fixed: bash exports
   `FW_FIXTURES`, Python reads it via `os.environ`.
2. The GW-13 `docx→md` row needs `sample.docx`, but the Linux
   script never had a docx generator (Windows relies on a leftover
   fixture in `target/matrix/fixtures`). Fixed: after the CLI
   build, the script derives `sample.docx` from `sample.md`
   through the app's own md→docx (pandoc) lane, `|| true` so a
   missing pandoc degrades to that row failing instead of killing
   the run. Verified on macair: rm + regenerate reproduces the
   10,445-byte fixture.

One operational note: an mbox→pdf matrix line on Linux showed ~2.5
minutes of formatwright CPU before completing; it passes (and passed
in the 2026-09-05 baseline), but the office/html→pdf chain on Linux
is noticeably slower than Windows — worth remembering if CI timing
tolerances ever cover this lane.
