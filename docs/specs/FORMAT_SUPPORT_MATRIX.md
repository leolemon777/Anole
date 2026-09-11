# Format and Platform Support Matrix

- Status: Phase 0 baseline
- Version: 0.1
- Updated: 2026-09-07 (GW-08 Document pack evidence)

## 1. Support labels

- Certified: all golden fixtures pass on the platform with a certified engine pack.
- Experimental: adapter exists, but the full fixture/platform matrix has not passed.
- Detected: Doctor can inspect the engine, but the workflow is not claimed.
- Unsupported: planning must reject the workflow.

Marketing and UI must use these labels. Engine-advertised formats are never automatically described as Certified.

## 2. Candidate v0.1 platform matrix

The following is the test target, not yet a support claim:

| Platform | Architecture | Alpha target | Public Beta gate |
|---|---|---|---|
| Windows 11 | x86_64 | Primary development platform | Installer, process-tree cancellation, long paths, signing |
| macOS | Apple Silicon | Phase 1 CI and Phase 4 desktop | Signed/notarized app, engine packs, Finder action |
| macOS | x86_64 | Build verification | Best-effort Beta unless full golden matrix is available |
| Ubuntu LTS | x86_64 | Phase 1 CI | AppImage, engine discovery, file-manager action |

Minimum exact OS releases must be frozen in ADR-0005 after Tauri, WebView, code-signing, and engine-pack tests. Windows 10 is not a default Public Beta target because its general support lifecycle has ended; a community build may remain possible.

## 3. Filesystem behavior

| Environment | v0.1 behavior |
|---|---|
| Local NTFS/APFS/ext4 | Required and tested |
| Removable local drive | Supported when staging and final output share a filesystem |
| Network share/UNC | Experimental; no atomic guarantee unless proven |
| Cloud-synced hydrated file | Best effort; source identity is rechecked |
| Cloud placeholder | Block until hydrated locally |
| Directory symlink | Not traversed by default |
| File symlink | Allowed only inside authorized input root |
| Hardlink | Read as a file; output hardlink identity is not preserved |
| Remote URL | Unsupported in v0.1 |

## 4. Workflow matrix

| Workflow | Inputs | Outputs | Primary engine | Initial status |
|---|---|---|---|---|
| GW-01 | HEIC, HEIF | JPG, PNG | libvips target; libheif development fallback | Experimental on Windows |
| GW-02 | PNG, JPG | WebP, AVIF | libvips (FFmpeg development fallback) | Experimental on Windows |
| GW-03 | Supported image directory | WebP, AVIF, JPG, PNG | libvips (FFmpeg development fallback) | Experimental on Windows |
| GW-04 | MOV, MKV, AVI, WebM | MP4 | FFmpeg | Experimental on Windows |
| GW-05 | Video containers with audio | MP3, M4A, WAV | FFmpeg | Experimental on Windows |
| GW-06 | Supported video | GIF | FFmpeg | Experimental on Windows |
| GW-07 | FLAC, WAV, MP3, AAC, M4A, OGG, Opus | Selected audio target | FFmpeg | Experimental on Windows |
| GW-08 | DOCX, PPTX, XLSX | PDF | LibreOffice (optional Document pack 26.2.6 or host install) + Poppler validation | Experimental on Windows |
| GW-09 | PDF | PNG, JPG | Poppler pdfinfo/pdftoppm | Experimental on Windows |
| GW-10 | Markdown, HTML, plain text, SVG | PDF, DOCX, EPUB | HTML/SVG→PDF: system-discovered Edge print + Poppler vector validation (preferred); Markdown/plain text keeps Pandoc + LibreOffice | Experimental on Windows (browser lane: formal sandbox evidence 2026-09-01, `scripts/test_browser_print_sandbox.ps1`) |
| GW-11 | CSV, JSON, YAML, XML | CSV, JSON, YAML, XML | Rust native | Experimental on Windows |
| GW-12 | Supported media/document | Cleaned copy | Type-specific adapter | Experimental media slice on Windows |
| GW-13 | DOCX, HTML/HTM, EML, MSG, MBOX, PDF, PNG/JPG/TIFF/BMP | Markdown (md) | DOCX/HTML→md: Pandoc (`--to=gfm`); EML/MSG/MBOX→md: Rust builtin adapter; PDF→md: Poppler `pdftotext` text layer; images→md: Tesseract OCR | Experimental on Windows |

Windows Starter Media（FFmpeg）为本机 GW-04/05/06/07 切片提供 Experimental 证据（沙箱 remux、Explorer Convert to MP4 等）。全部行仍非 Certified：干净机 / 全 fixture / 签名包未关闭。

GW-10 的浏览器打印 lane（ADR-0012）：HTML/HTM 与新增 SVG 输入在开发构建下经系统发现的 Edge 无头打印产出矢量 PDF，并用 pdfinfo/pdftoppm/pdftotext/pdffonts 验证（文字层可提取、字体全内嵌）；HTML 保留 Pandoc lane 作为回退，SVG 仅此 lane。Release 构建仍需激活已验证引擎包。

GW-08 的引擎来源（2026-09-07，E-04）：可选 Document pack `anole-document` v26.2.6（官方 TDF MSI 在 Linux 解包组 pack，MPL-2.0，19,476 文件 SPDX SBOM）提供 pack 内 `soffice.com`，或继续使用宿主自装 LibreOffice。docx/xlsx→PDF 已由 pack 自有引擎真跑验证（`pdftotext` 文本回读一致、用户 LibreOffice 配置树零污染）；pptx 合成 fixture 无文字层属 fixture 限制（pack 与系统 LibreOffice 行为一致），真实 pptx 验证随 E-12 / 发布烟测。GW-08 全行仍非 Certified。

GW-13 的 Markdown 导出波（2026-09-08）：直连覆盖 DOCX/HTML→md（Pandoc，`--sandbox=true` + `resource_policy=deny-all`，HTML 输入含外部资源时 PolicyBlocked）、EML/MSG/MBOX→md（内置 Rust 适配器，`# 主题` + 加粗头字段 + 正文，邮件分隔标记与 txt/html 同构）、PDF→md（Poppler `pdftotext` 文本层提取，`loss_class=Lossy`——标题/表格/版式结构不保留，多栏阅读顺序为 Unknown）、图像 OCR→md（与 →txt 同一 Tesseract lane，识别文本装入 .md）。pptx/xlsx/odt/odp/rtf/svg 经 PDF 中转在 CLI 链式可达（`X→pdf→md`）。音频转录、YouTube、EXIF 元数据等 MarkItDown 式源明确不在范围（与本地优先/零网络定位冲突）。

GW-14 的 XLSX 数据导出 + 桌面链暴露（2026-09-10，Leo "全部都要"）：xlsx→csv 直连路由（soffice `csv:Text - txt - csv (StarCalc)` filter 导出激活 sheet；多 sheet 丢弃、公式写为计算值在 plan 的 dropped/changed 如实声明；验收 OFFICE_CSV_OPENS/ROWS_PRESENT/FIELDS_PRESENT 为内置宽松 CSV 解析，无 Poppler 依赖；老 xls 仍不支持）。桌面 GUI 同步暴露 CLI 已有的两跳链：`desktop_capability_snapshot` 对无直达路线但链可达且链上引擎齐备的目标标记 available（message 注明 Two-step conversion），preview 对 Unsupported 目标回落到链（preview 显示第一段 plan + chain 提示），运行走新命令 `run_desktop_chained_conversion`（`execute_conversion_chain` 每段独立验收，approved_plan_hash 与第一段 plan hash 强校验，不进 job 队列——queue 对链目标诚实拒绝）。CLI/server 的 snapshot 语义不变。e2e：xlsx→csv 与链式 xlsx→jpg（经 PDF 分页目录）真引擎全 Pass。

GW-15 同日升级（Leo "全部一起开始"）：xlsx→csv 引擎换为内置 `anole.office-csv`（calamine 0.36.1 纯 Rust）——**全工作表**导出为分页目录（`sheet-NN[-名称].csv`，Unicode 工作表名保留），引擎需求清零（无 LibreOffice 也可用），验收加 OFFICE_CSV_SHEET_COUNT（sheet 数守恒）；soffice 版仅存活数小时即被替换。链式补全：入队（plan.constraints 携带链元数据 + 请求快照，队列 worker 重建整链惰性执行）、立即链式运行的取消与合成 job 事件（运行态/取消按钮照常）。mbox 家族支持 mboxcl（Content-Length 字节精切，不符即拒）与 mboxo（保守假设不 unescape，probe 记录 variant）。Web 轨道 W1：`/v1/uploads*` + `/v1/jobs*` 上传-转换-下载流（50MB 上限、TTL 清扫、每 IP 单活跃作业、路径注入剥离、分页输出懒 zip）+ `apps/web` SPA + 三阶段 Dockerfile。

## 5. MP4 planning baseline

Dynamic engine inspection remains authoritative, but the first planner fixture uses:

- H.264/AVC video: remux candidate.
- H.265/HEVC video: remux candidate subject to selected compatibility profile and tags.
- AAC audio: remux candidate.
- MP3 audio: allowed only when the selected MP4 profile accepts it; otherwise planned audio transcode.
- VP8/VP9 video: transcode candidate.
- Opus audio: explicit compatibility decision; never silently retained.
- Text subtitles: convert, drop, or externalize only when shown in the Plan.
- Image-based subtitles: do not silently discard.

## 6. Engine certification record

Each certified row records:

- Engine and semantic version.
- Binary SHA-256.
- Build configuration.
- Platform and architecture.
- Capability manifest hash.
- Fixture suite revision.
- Pass date.
- Known warnings.
- License review ID.

## 7. Promotion rule

A status moves from Planned to Experimental when the adapter and at least one end-to-end fixture pass. It moves to Certified only after all required fixtures and release gates pass on the named platform.
