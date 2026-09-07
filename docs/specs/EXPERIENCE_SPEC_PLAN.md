# Anole 体验收口 Spec Plan（v0.1.1 / v0.2 增量）

**文档类型：** 产品规格 + 技术规格 + 交付计划（v0.1.0 发布后的用户体验收口增量）
**作者：** Anole maintainers（草稿，待负责人审查）
**日期：** 2026-09-06
**状态：** Draft — 等待负责人评审（含 6 个 DECISION 拍板点）
**权威边界：** 产品范围与发布门槛以仓库根目录 [`SPEC_PLAN.md`](../../SPEC_PLAN.md) 为准；执行顺序以 [`VOC_BACKLOG.md`](../VOC_BACKLOG.md) 为准；Gate / R-xxx 关闭条件以 [`MASTER_EXECUTION_PLAN.md`](../MASTER_EXECUTION_PLAN.md) 为准。本文件只定义**使用体验收口**这一增量（编号 E-xx），与 G-xx（竞品差距）、R-xxx（缺陷）、VOC 波次交叉时互相引用、不重复定义。

---

## 一句话增量陈述

**v0.1.0 的功能底盘已经足够厚；本增量不再加格式、不再加入口形态，只把「新用户第一印象、Windows 日常手感、结果信任感」三段体验收口到可交给熟人长期使用的程度。**

---

## Overview

截至 2026-09-06，桌面端已具备 7 页导航（Convert/Jobs/Presets/Engines/Reports/Maintenance/Settings）、拖放（含目录自动切文件夹批量）、空状态三卡、Plan 大白话、转换成功/入队/失败系统 toast、打开输出位置、持久队列的暂停/恢复/批量动作、启动恢复横幅、Windows 经典 Explorer 右键（Open-in + 17 个按扩展名 Convert verb）、双语、WebView 自动无障碍基线与 updater。**VOC 第 1 波已全部落地（本文档验证过代码），但 VOC_BACKLOG（2026-08-18）尚未勾选。**

剩余缺口按用户旅程分三波，全部来自三份既有待办（VOC 第 2/3 波、MASTER Gate 2 未完项、COMPETITIVE_GAP_ROADMAP 剩余项）加两条新增项（E-09 输出预览、E-10 暗色模式），无一凭空发明。

### 现状与证据基线（2026-09-06）

| # | 事实 | 证据路径 |
|---|---|---|
| 1 | v0.1.0 安装包 unsigned，SmartScreen 会警告 | `README.md` Known gaps；`docs/release/WINDOWS_PACKAGING.md`（NotSigned） |
| 2 | Explorer 集成仅经典菜单；Win11 需「显示更多选项」 | `apps/desktop/src-tauri/explorer-verbs.json`（17 个 Convert verb 全部经典注册）；MASTER Gate 2 未完项 |
| 3 | Starter 仅 `pdf` + `media` 两个 pack；HEIC 靠开发机 MSYS2 手搓 heif-dec，不可分发；Office→PDF 依赖用户自装 LibreOffice | `docs/testing/WINDOWS_STARTER.md`；`docs/VOC_BACKLOG.md` 3.1/3.3 |
| 4 | 右键无 Directory Convert verb；`ShellConvertCoordinator` 支持多选同目标合并（800ms quiet / 32 上限）但**不接受目录输入** | `apps/desktop/src-tauri/src/shell_convert.rs`；`explorer-verbs.json` 仅有 `Directory\shell` Open-in |
| 5 | 右键动词表写死在 `explorer-verbs.json`，NSIS 安装时静态注册、卸载精确删除自有键 | `apps/desktop/src-tauri/windows-explorer-hooks.nsh` |
| 6 | 加密 PDF 无密码入口，直接失败；规格早已写明「密码不进 Plan/日志/SQLite」 | `docs/VOC_BACKLOG.md` 3.4 |
| 7 | 进度区有真实阶段与调度等待原因，刻意不显示速率/ETA | MASTER §1.1；`App.tsx` 队列投影 |
| 8 | OCR 代码就绪、引擎按用户决定暂缓安装（Windows 无 host Tesseract 即不可用） | `docs/COMPETITIVE_GAP_ROADMAP.md` Wave 4 状态行（G-24） |
| 9 | 无真实用户研究记录；UX 结论全部来自 VOC 推断 + 自动无障碍基线 | MASTER Gate 5 未开始 |
| 10 | `styles.css` 单主题 Meadowlark，无暗色模式；转换结果在应用内无预览，需跳文件管理器 | `apps/desktop/src/styles.css` |

---

## Goals & Non-Goals

### Goals

1. 新用户下载→安装→第一次转换成功，全程无「这软件可信吗 / 菜单没装上 / 缺引擎看不懂」卡点。
2. Windows 日常高频动作（右键单转、多选转、整夹转、按自己的习惯改右键）达到 FileConverter 级手感。
3. 转换结果「看得见」：实测速率、输出预览、加密文件可解，信任感建立在应用内而不是文件管理器里。
4. 以上每一项带可复现验收，沿用「每操作产出机器可读验收证明」的项目纪律。

### Non-Goals（本增量明确不做）

- 拼格式数量、对标 5,438 路由（VOC 实站注记禁止）。
- API / MCP / Docker 自托管 / Web WASM（VOC 第 5 波禁止；web 服务方向另行讨论，见负责人笔记）。
- macOS Finder / Linux 文件管理器集成（Windows 菜单稳定之前不做；macOS 仍仅 CI 覆盖，对外宣传收着说）。
- PDF→DOCX 可编辑还原、Ghostscript（AGPL 未决）、真矢量化（potrace GPL）——沿用 COMPETITIVE_GAP_ROADMAP §6 结论。
- 引导用户安装系统 `PATH` 上的 FFmpeg/LibreOffice（与 Release 不扫 PATH 的立场冲突）；引擎补齐只走官方 starter pack 路线。

---

## 编号总览

| 编号 | 内容 | 来源 | 波次 | 规模 | 硬依赖 |
|---|---|---|---|---|---|
| E-01 | Authenticode 签名 + 签名发布流 | G-04 / VOC 4.3·4.5 | U1 | M | **DECISION-1（证书）** |
| E-02 | Windows 11 现代顶层菜单 | VOC 2.1 / MASTER Gate 2 | U1 | L | E-01（签名身份） |
| E-03 | 官方 Image/HEIC 包（解码优先） | VOC 3.1 | U1 | M | **DECISION-2（分发方案）** |
| E-04 | Document Starter Pack（Office→PDF） | VOC 3.3 | U1 | M–L | **DECISION-3（LO 法律结论）** |
| E-05 | 文件夹右键整夹转换 | VOC 2.2 | U2 | M | 无（可与 E-02 并行） |
| E-06 | 右键动词/预设可配置（运行时注册） | VOC 2.3 | U2 | M | 无 |
| E-07 | 加密 PDF 密码框（安全通道） | VOC 3.4 | U2 | M | 无 |
| E-08 | 引擎专用实测速率 | MASTER Gate 2 | U2 | S–M | 无 |
| E-09 | 输出预览缩略图 | 新增（本次评估） | U3 | S–M | 无 |
| E-10 | 暗色模式 | 新增（本次评估） | U3 | S | 无 |
| E-11 | OCR 引擎决策与安装 | G-24 | U3 | M（代码就绪） | **DECISION-4（Tesseract）** |
| E-12 | 真人 first-run 用户研究（R1） | MASTER Gate 5 前哨 | U3 | S | E-01（签过名的包才给真人） |

规模标尺沿用 COMPETITIVE_GAP_ROADMAP：S ≤ 3 天 / M 1–2 周 / L ≥ 3 周（单人）。
Clean-VM 认证证据（VOC 4.2 / R-008·R-009 Closed）已有专属脚本与文档，本文件只引用不重定义。

**执行状态（2026-09-07 第三波收口）：** E-05、E-06、E-07、E-08、E-09、E-10、E-11（OCR pack 三件套落地）、E-04（Document pack 实体落地：官方 LO 26.2.6 MSI 在 Linux 无执行解包、19,476 文件 SBOM、真跑 docx→pdf 通过、发布 zip 哈希已固定并激活下载按钮待挂资产）已实现并验证；E-02 已交付过渡缓解，完整 sparse-MSIX 菜单等 DECISION-1 证书到账后开工；E-03 已完成供应链调查，等清单 A 批复后解包验证 import 表并组 pack；E-01 CI 骨架就绪、CA 采购为 Leo 动作；E-12 任务单就绪、等 Leo 排 3–5 名测试者。已知偏差（E-07 密码 argv、E-09 无缓存、E-06 reg.exe 注册）见 implementation-notes 与 SECURITY.md。

---

## Wave U1 — 第一印象（发布阻断级）

### E-01 Authenticode 代码签名与签名发布流

**现状：** v0.1.0 NSIS 未签名；SmartScreen 蓝色警告是新用户第一秒的最大流失点。updater 签名密钥 2026-09-03 已重建并有 scratch-sign 验证流程，但安装包本身无 Authenticode。

**方案：**
1. 负责人完成证书采购（DECISION-1）；密钥进 CI secrets 的路线沿用 updater 密钥的教训：**只经 `printf | gh secret set`，绝不经 cmd shim**（见实现笔记 2026-09-03 事故）。
2. `release-candidate.yml` 增加签名步骤：`signtool sign /fd SHA256 /tr <TSA> /td SHA256` 对 `formatwright-desktop.exe`、NSIS 安装器、uninstaller 依次签名；签名后 `signtool verify /pa /all` 断言。
3. `docs/release/WINDOWS_PACKAGING.md` 与 `RELEASE_CHECKLIST.md` 增加签名档位列；`SHA256SUMS` 签名后重生成。

**验收：**
- 干净 Win11 VM 上双击安装器无 SmartScreen 警告（EV）或仅首次信誉警告（OV，需记录预期）。
- `Get-AuthenticodeSignature` 对三件产物均 `Valid`，时间戳存在。
- 升级回滚烟测在签名包上重跑通过（updater 链路不回退）。

**风险：** OV 证书 SmartScreen 信誉需要下载量积累，「立即无警告」不保证——在发布说明中如实标注。2024 年后 EV 也不再承诺即时通过，DECISION-1 要按预算而非按承诺选。

**拍板：DECISION-1** — 证书类型（OV vs EV）、CA 供应商、预算、证书存放（HSM/云签名 vs 本机 PFX + CI secret）。

### E-02 Windows 11 现代顶层菜单

**现状：** 17 个 Convert verb + Open-in 全部经典注册；Win11 用户右键需按「显示更多选项」（或 Shift+F10），第一手感是「菜单没装上」——正是 FileConverter 被骂最多的点。

**技术事实：** Win11 顶层上下文菜单只渲染 `IExplorerCommand` COM 实现；纯注册表 verb 一律折叠进「显示更多选项」。社区通行路线（PowerToys、Files）是 **sparse MSIX package + IExplorerCommand**：一个小型 COM DLL（Rust `windows-rs` 实现 `IExplorerCommand`，或嵌入式 C++/C# 组件）+ `AppxManifest`（sparse），由安装器 `Add-AppxPackage -ExternalLocation` 注册。**Publisher 必须与签名证书主体一致 → 硬依赖 E-01。**

**方案：**
1. `apps/desktop/src-tauri/win11-menu/`（新增）：`IExplorerCommand` 实现，verb 集从 E-06 的运行时配置读取（先落地经典版数据源，E-02 只换渲染层），点击后以现有 `--shell-convert --to` / `--shell-open` 语义转发到单实例。
2. NSIS 增加 sparse package 注册/卸载步骤；`test_windows_explorer_integration.ps1` 增加顶层菜单断言（Win11 VM 上 `Get-AppxPackage` + 顶层 verb 可见性）。
3. E-02 落地前的**过渡缓解**（独立小改，先行合入）：设置页与 Doctor 增加一句 Win11 提示文案「右键菜单在『显示更多选项』里，或按住 Shift 右键」。

**验收：**
- Win11 干净 VM：安装后右键 `.pdf` 顶层直接出现 Anole / Convert to PNG，无需「显示更多选项」。
- 卸载后 sparse package 与 COM 注册零残留；升级（updater）后菜单仍在（VOC 2.4 语义）。
- 经典菜单（Win10 / Win11 显示更多选项）行为不回退，现有烟测全绿。

**风险：** Rust COM 组件工作量高于预期（L 的主因）；可降级为嵌入式小型 C# DLL（引入 .NET 依赖需在 ADR 中记取舍）。MSIX 的 per-user 注册在域环境可能有组策略限制——烟测矩阵加一台域加入场景或明确不支持并写文档。

### E-03 官方 Image/HEIC 包（解码优先）

**现状：** HEIC 转换路由已落地，但 Release 引擎依赖开发机 MSYS2 手搓的 heif-dec（含 libx264 GPL 组件 dll），**不可分发**；干净机 HEIC→JPG 实际不可用。这是「中文用户搜得最多的格式」（iPhone 照片）。

**方案：**
1. 法律路线先行（DECISION-2）：`libheif`（LGPL-3.0）+ `libde265`（LGPL-3.0）动态链接组合用于 **HEIC 解码**（→JPG/PNG/WebP）合规；**HEIC 编码**（→HEIC）依赖 x265（GPL）暂不做，除非找到非 GPL 编码器。结论写入 `engines/README.md` 登记流程（manifest / 许可证 / 哈希），沿用 ADR-0011 keyring。
2. 构建 `dist/engine-packs/windows-x86_64/starter/image/`（第三 pack）：官方可分发 heif-dec + manifest + 哈希清单；首启激活与能力门控走既有 starter 机制。
3. HEIC 元数据：拍摄时间保留（VOC 3.2 验收：100 张 HEIC 输出日期不是「今天」）；地点/ICC 按 Plan 明示保留或丢弃。

**验收：**
- 干净离线 VM：装官方包后 HEIC→JPG/PNG 走通，输出 EXIF DateTimeOriginal 保留，验证报告齐全。
- `engines verify` 对 image pack manifest 全绿；starter 断言（非空 `dist/engine-packs`）覆盖第三 pack。
- 批量 100 张 HEIC，页数守恒、日期保留，报告逐项 Pass。

**风险：** LGPL 动态链接义务（提供目标源码链接/许可证文本随包分发）需在 NOTICE 与包内 LICENSE 文件落实；libde265 专利主张（HEVC 专利池）历来存在但面向解码免费分发的先例多——DECISION-2 一并确认。

**拍板：DECISION-2** — HEIC 解码包的分发方案（libheif+libde265 LGPL 动态链接 vs 寻找商业授权 vs 放弃 HEIC 只做专利风险更低格式）；专利风险接受度。

### E-04 Document Starter Pack（Office→PDF）

**现状：** docx/xlsx/pptx→PDF 路由依赖用户自装 LibreOffice；没装的用户看到的是灰目标 + 说明文字，没有「点一下补齐能力」的路。VOC 明确禁止引导装 PATH 引擎，唯一合规路线是官方 pack。

**方案：**
1. DECISION-3 先给 LibreOffice 可再分发结论（MPL-2.0 主体 + LGPL 组件：源码链接义务、LICENSE 保留、商标改号问题——LibreOffice 官方 FAQ 允许再分发但有条件）。
2. 构建 `dist/engine-packs/windows-x86_64/starter/document/`：soffice + 隔离用户 profile（不污染用户已有 LO 配置）+ manifest。包体积 ~350MB，考虑拆为**可选 pack**：安装器内勾选下载或安装后应用内激活（保持安装器本体苗条——具体形态归 DECISION-3）。
3. `.xls` 老格式维持现有白话文案（叫用户另存 xlsx），不扩范围。

**验收：**
- 干净离线 VM：新装即转 docx/xlsx/pptx→PDF，每份带验证报告；用户机器上已有 LibreOffice 时，pack 的隔离 profile 与用户配置互不干扰。
- `engines verify` 全绿；doctor 对 pack 内 soffice 报 Trusted（签名链路依赖 E-01 keyring 体系）。
- 首启激活 + 升级不重复安装（版本化 install 机制沿用 starter 现有实现）。

**风险：** 350MB 下载对「离线包」哲学的冲击——若做成可选下载，需要在隐私文档写清「应用只从官方 release URL 拉取、带哈希校验、可断网跳过」，否则与 local-first 叙事冲突。LibreOffice 大版本升级跟随策略需写入维护文档。

**拍板：DECISION-3** — LibreOffice 打包法律结论 + 包形态（内嵌 / 可选下载 / 两者）；接受包体积与否。

---

## Wave U2 — 日常手感

### E-05 文件夹右键整夹转换

**现状：** `explorer-verbs.json` 仅有 `Directory\shell\FormatWright` Open-in；`ShellConvertCoordinator` 不接受目录输入。拖目录进窗口可以整夹批量（FolderPreview + 磁盘预算 + 原子入队），右键不行。

**方案：**
1. `explorer-verbs.json` / `windows-explorer-hooks.nsh` 增加 `Directory\shell\FormatWright.ConvertTo…` 系列动词（推荐目标集合与文件类一致：图片夹→JPG/WebP、PDF 夹→PNG 等；枚举哪几个动词在实现前以「文件夹内容主体类型」抽样建议定稿——不静默全注册 17 个，避免菜单爆炸，**定稿清单实现前给负责人过目**）。
2. `shell_convert.rs`：目录输入放行,新增 `classify_directory_for_convert`（浅层枚举上限如 10,000 项防深目录卡死）→ 按多数可转类型选目标 → 复用桌面 FolderPreview 的磁盘预检与原子入队；异构不可转文件**显式列入 skipped 清单**（toast + Jobs 页可见），不静默失败。
3. 右键整夹 = 批准语义（与单文件 Convert to X 同等例外，KD-2 沿用），但仍走持久队列 + 验证 + no-clobber。

**验收：**
- 资源管理器右键含 10 jpg + 1 txt 的相册夹 → Convert to WebP：10 项入队完成、txt 出现在 skipped 报告、零覆盖、磁盘预检在剩余空间不足时拒绝入队并给白话原因。
- 深层/超限目录（>10,000 项）给出明确拒绝文案而非卡死。
- 卸载清理新增 Directory verb 自有键；烟测脚本断言同步扩展。

**风险：** 目录动词与其他软件 Directory shell 键的冲突（自有键名空间内可控）；推荐目标的启发式可能不符合用户预期——首次执行弹出的确认摘要（复用 plan-first 预览横幅）兜底。

### E-06 右键动词/预设可配置（运行时注册）

**现状：** 17 个 verb 写死在 `explorer-verbs.json`，NSIS 安装时静态注册 HKLM；用户不能改默认（VOC 2.3「改一次，右键跟着变」未达）。

**方案：**
1. 注册职责迁移：NSIS 只保留 Open-in 两把键（回退保底）；全部 Convert verbs 改由**应用首启/设置页写入 HKCU**（`HKCU\Software\Classes\<ext>\shell\FormatWright.*`），增删改即时生效。
2. Settings 页新增「右键菜单」区：每个动词可改目标格式/绑定预设（默认「小 JPG」「PDF 每页 PNG」等 PresetLibrary 预设）、可启停、可恢复默认；导出/导入随 PresetLibrary 现有通道。
3. E-02 的 Win11 顶层菜单从同一数据源读取（避免两套配置）。

**验收：**
- 改默认动词为自定义预设后，资源管理器右键（经典与 E-02 顶层）立即反映，无需重装。
- 两台机器间导出→导入还原右键配置；「恢复默认」回到 17 verb 基线。
- 卸载后 HKCU 自有键精确清除（卸载器删除 per-user 键需在 NSIS 卸载段 + 应用内「移除集成」双保险）；多用户机器互不影响。
- 现有烟测脚本从断言 NSIS 静态键改为断言首启后 HKCU 键。

**风险：** 行为变更幅度大（安装时菜单不再立即可用，要等首启）——缓解：安装器末尾静默启动一次应用完成注册（现有 `--shell-open` 冷启动路径复用）；注册表迁移对已装用户做一次性搬家并留 journal。

### E-07 加密 PDF 密码框

**现状：** 规格早有（「密码不进 Plan/日志/SQLite」），未实现；加密文件直接引擎失败。

**方案：**
1. `probe` 已能识别加密（`pdfinfo` 报 Encrypted）→ Convert 页出现专用密码框（`type="password"`，带「不保存」白话说明），CLI 加 `--password`（读 tty 或 `--password-file`，**不接受明文 argv**）。
2. Core：engine-sdk 子进程协议新增受限 secret 通道——密码经**每进程一次性环境变量**传入（argv 会泄漏到进程列表，禁用）；PlanRequest 无密码字段，`plan_hash` 计算时以 `has_password: bool` 替代真值；报告/日志/SQLite 同样只记布尔。
3. 支持范围：qpdf 解密/加密 lane + PDF 输入类转换（poppler 系均接受密码 env/参数映射时仍走 env）。

**验收：**
- 加密 PDF→PNG / PDF→解密回读 全链路成功；错误密码给白话文案（不暴露是否文件存在）。
- 对 SQLite 库文件、全部日志、报告 JSON、Plan JSON 做**字符串扫描，密码零命中**（新增自动化负向测试）。
- 崩溃转储风险记录：默认 Windows 错误报告禁用下 minidump 不落盘；在 SECURITY.md 增补密码与内存转储的残留风险说明。

**风险：** 环境变量在 `/proc` 类接口下对同 UID 进程可见（Windows 上 `NtQueryInformationProcess` 门槛较高，风险可接受并记录）；密码在内存中即用即弃但不承诺 `zeroize`（记入 SECURITY.md，不夸大）。

### E-08 引擎专用实测速率

**现状：** 队列显示真实阶段与调度等待原因，刻意不伪造速率/ETA（正确）；但长任务（10k 混合、大视频）等待感盲，用户无法判断「卡了还是在跑」。

**方案：**
1. `JobExecutionService` 每任务记录引擎墙钟 + 输入/输出字节数 → 按 (engine, capability) 维护滚动吞吐统计（SQLite 内持久，随 MaintenanceService 备份）。
2. UI：运行中任务显示**实测吞吐**（如 `4.2 MB/s · 已完成 137/9600`）；仍**不显示 ETA**，或仅当同构任务 ≥10 个已完成时显示保守区间并标注「基于历史实测」。
3. 调度等待（排队非运行）时显示等待原因，不显示任何速率——维持「不伪造」纪律。

**验收：**
- 10k 混合负载中 UI 速率与实际吞吐误差 <10%（对照 release gate 已有的 jobs/s 统计）。
- 排队任务不出现速率；统计写入随 SQLite 备份/恢复往返一致。

**风险：** 低。统计维度选择 (engine, capability) 还是 (route) 影响颗粒度，实现时以现有 JobEvent 数据先行试算一次再定。

---

## Wave U3 — 信任与打磨

### E-09 输出预览缩略图（新增项）

**现状：** 转换结果在应用内不可见，「转得对不对」要出窗到文件管理器确认；与「验证报告」文化的配合缺最后一屏。

**方案：**
1. 成功横幅与 Reports 详情页增加输出预览：图片目标直接缩略；PDF 目标 `pdftoppm` 首页 256px；视频目标第一帧抽帧（`ffmpeg -frames:v 1`，二期可做）。
2. 预览生成走既有沙箱 subprocess 纪律；缓存于 LOCALAPPDATA 临时目录（随维护中心清理）；**引擎缺失时优雅隐藏**该区块（不报错、不占位、不诱导装引擎），维持能力门控风格。

**验收：**
- PDF→PNG 成功后 Reports 详情显示输出首页缩略图；点击仍走「打开输出位置」。
- 无 poppler / 无 ffmpeg 环境下该区块隐藏且页面无报错（负向测试）。
- 预览缓存随 Maintenance 清理回收；重建预览不重复转换。

**风险：** 大图/大 PDF 预览生成的 CPU 峰值——限制尺寸（256px）与并发（1）即可。

### E-10 暗色模式（新增项）

**现状：** `styles.css` 单 Meadowlark 主题；长队列监控用户对暗色有常规预期。

**方案：**
1. 主题变量化：现色值全部抽为 CSS custom properties（`--fw-*` 前缀）。
2. 三态切换：System（`prefers-color-scheme`）/ Light / Dark，Settings 手动覆盖，持久化到 ApplicationSettings（随状态整包备份恢复）。
3. 现有无障碍基线全量重跑（高对比 / forced-colors / 200% 缩放 / RTL）确保不回退。

**验收：**
- 三态切换即时生效且重启保持；整包备份→恢复后主题选择一致。
- 自动无障碍与高对比基线测试全绿（沿用现有 WebView 基线脚本）。

**风险：** 低。注意品牌锁字（branding/final）在暗色下的对比度需人工过一遍。

### E-11 OCR 引擎决策与安装（G-24 携带）

**现状：** OCR 管线代码就绪、验收标准已定义（`pdftotext` 非空 + 抽样词命中）；引擎安装按用户决定暂缓。当前 Windows 用户开箱无 OCR。

**方案：** DECISION-4 拍板后二选一：(a) Tesseract 入第五个 starter/可选 pack（引擎 Apache-2.0，训练数据许可单独盘——`eng.traineddata` Apache-2.0，中文数据需逐一核对）；(b) 维持「doctor 能发现 host 安装的 Tesseract，文档给官方安装指引」现状。选 (a) 则走 E-03 同款 pack 流程。

**验收：**（选 (a) 时）干净机扫描件→可搜索 PDF，OCR 验收组全绿；(b) 则 doctor 指引文案 + 文档更新即可关闭。

**拍板：DECISION-4** — 是否做 OCR pack；做的话训练数据语言集（eng / chi_sim / chi_tra）与许可核对范围。

### E-12 真人 first-run 用户研究（R1）

**现状：** 全部 UX 结论来自 VOC 推断与自动无障碍基线；零真实新用户记录（MASTER Gate 5 未开始）。

**方案：**
1. 脚本化任务单：干净 Win11 VM + 签名安装包，5 项任务——安装、拖 PDF 转图、右键单转、拖文件夹批量、在应用内找到验证报告并说出「转对了没有」。
2. 3–5 名非开发者（熟人即可）；记录完成率、卡点时间、文案不懂处；不设通过/失败门禁，产出发现清单进 `docs/testing/USER_STUDY_R1.md`，P0 发现回流本文件或 DEFECT_REGISTER。
3. 与 clean-VM 认证（VOC 4.2）共享 VM 快照准备工作，但不合并目的：认证是无人值守证据，用户研究是有人观察。

**验收：** 报告入档，每个 P0/P1 发现有去向（本文件新增 E-xx 或 DEFECT_REGISTER 条目）。

---

## 负责人拍板清单（工程不自行假设，全部有依赖项挂起）

> **2026-09-07 决议（Leo，"ok，按推荐"）**：DECISION-1 按推荐（OV + CA 云签名 KSP，约 $130–300/年；CA 采购与主体验证流程由 Leo 本人执行，工程侧 CI 骨架已就绪）；DECISION-2 选 a（LGPL 自建 image pack，接受 HEVC 专利理论风险并发布页披露）；DECISION-3 选 a（LibreOffice 可选下载 pack，PRIVACY 披露联网）；DECISION-4 选 a（Tesseract + eng + chi_sim 入第五 pack）；DECISION-5 维持 2 条目录动词；45 文件改动获批按 Conventional Commit 提交。

| # | 决策 | 阻塞 | 备注 |
|---|---|---|---|
| DECISION-1 | Authenticode 证书类型（OV/EV）、CA、预算、私钥存放 | E-01 → E-02 | EV 不再保证即时过 SmartScreen；按预算与信誉策略选 |
| DECISION-2 | HEIC 解码包分发方案与专利风险接受度 | E-03 | LGPL 动态链接可行；HEVC 专利池先例多但需确认 |
| DECISION-3 | LibreOffice 打包法律结论 + 包形态（内嵌/可选下载） | E-04 | 影响安装器体积与 local-first 叙事 |
| DECISION-4 | Tesseract 是否入 pack + 训练数据语言集 | E-11 | 引擎 Apache-2.0；中文训练数据逐一核对 |
| DECISION-5 | E-05 整夹转换的目录动词清单（图片/PDF/…哪几个） | E-05 实现前过目 | 防菜单爆炸 |
| DECISION-6 | E-02 的 COM 组件技术路线（Rust windows-rs vs 嵌 C# DLL） | E-02 详细设计 | Rust 无新依赖但工时高；C# 快但引 .NET |

---

## 顺序建议与依赖图

```text
E-01 (DECISION-1) ──→ E-02 (DECISION-6)          [签名身份是 MSIX Publisher 前提]
E-05 ┐
E-06 ┼─ 互相独立，先于/并行于 E-02 均可（经典菜单先受益）
E-07 ┘
E-03 (DECISION-2) / E-04 (DECISION-3) / E-11 (DECISION-4)   [各自拍板后随时插入]
E-08 / E-09 / E-10   [小项，穿插空档]
E-12 最后（需要 E-01 签名包；与 clean-VM 共享快照准备）
```

建议日历（1 人 + AI）：本周 E-01 签名流 + E-10 → 下两周 E-05 + E-06 → 第 4 周 E-02 → 拍板到位的引擎 pack（E-03/E-04/E-11）插 CI 空档 → 收尾 E-08/E-09 → E-12。

---

## 测试与文档同步清单（每项落地时随改，防再欠账）

- `scripts/test_windows_explorer_integration.ps1`：E-05 目录 verb、E-06 HKCU 注册时机、E-02 顶层菜单断言。
- `docs/testing/WINDOWS_STARTER.md`：E-03/E-04/E-11 新 pack 的 verify 步骤与哈希。
- `docs/release/WINDOWS_PACKAGING.md` / `RELEASE_CHECKLIST.md`：E-01 签名档位；E-02 sparse package 注册/卸载步骤。
- `docs/specs/UX_FLOWS.md`：E-05/E-07/E-09 新流程；`SECURITY.md`：E-07 密码残留风险；`PRIVACY.md`：E-04 可选下载的联网行为。
- `docs/VOC_BACKLOG.md`：勾选第 1 波（已落地未勾），并按本文件更新第 2/3 波条目；`docs/MASTER_EXECUTION_PLAN.md` 对应 Gate 勾选。
- `docs/specs/FORMAT_SUPPORT_MATRIX.md`：HEIC/Office 行去掉「依赖开发机引擎」类 caveat，换 pack 证据。

## 风险总表

| 风险 | 等级 | 缓解 |
|---|---|---|
| E-02 COM/MSIX 工期失控 | 高 | 先交付过渡文案 + 经典菜单不回退；E-02 单独 milestone，可整体后延不阻塞其他项 |
| 引擎 pack 法律审查（HEVC 专利 / LGPL 义务 / LO 商标）拖期 | 高 | DECISION-2/3 尽早启动，审查期其余 Wave 照常推进 |
| E-06 注册时机迁移影响已装用户 | 中 | 一次性搬家 + journal + 安装器末尾静默首启注册 |
| 密码内存残留 | 中 | 即用即弃 + SECURITY.md 如实披露，不承诺 zeroize |
| 证书 SmartScreen 信誉积累慢（OV） | 中 | 发布说明如实标注；优先引导下载渠道信誉 |
