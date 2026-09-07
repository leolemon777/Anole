# DECISION-2/3/4/5 拍板简报：引擎包决策材料

**文档类型：** 负责人拍板材料(spec `EXPERIENCE_SPEC_PLAN.md` DECISION-2/3/4/5)
**日期：** 2026-09-07(事实核查日)
**状态：** 等待 Leo 拍板。与 `CODE_SIGNING_DECISION_BRIEF.md`(DECISION-1)一起,回四个"按推荐"即可全部解锁工程侧。

---

## DECISION-2 — HEIC 官方 Image 包(E-03)

**现状**:HEIC→JPG/PNG 路由已实现,但 Release 引擎是开发机手搓 heif-dec(含 GPL 的 libx264),不可分发;干净机 HEIC 开箱不可用。

**事实**:
- `libheif` 与 `libde265`(HEVC 解码)均为 **LGPL-3.0**:动态链接 + 随包携带许可证文本 + 注明修改 + 提供对应源码链接,即满足义务。GIMP/darktable 等开源项目走同一路线分发 HEIF 解码。
- HEVC 专利池(Access Advance、MPEG LA/Velos)在**理论**上覆盖 HEVC 解码器分发;实践中对开源终端解码器分发的追索先例极少,但**这不是无风险结论**——这是"接受理论风险 vs 放弃格式"的取舍,只有你能拍。
- HEIC **编码**需 x265(GPL),排除;只做解码方向(→JPG/PNG/WebP)。

**选项**:
a) **libheif + libde265 LGPL 动态链接,自建 image starter pack(推荐)** — 成本最低,与同类开源软件实践一致;风险=HEVC 专利池的理论主张,发布页如实披露。
b) 购买商业 HEVC 解码授权后打包 — 消除专利风险,成本与谈判周期对个人开发者不现实。
c) 放弃 HEIC,保留 host 发现 — 零风险,但"iPhone 照片"是中文用户搜索最多的转换需求。

**拍板**:___(回 "a/b/c")

## DECISION-3 — Document Starter Pack(E-04)

**现状**:Office→PDF 依赖用户自装 LibreOffice;没装的用户只见灰目标。

**事实**(2026-09-07 核查):
- LibreOffice 主体 **MPL-2.0**:再分发未修改的官方二进制合法,义务 = 携带许可文本 + 源码可得性声明。
- **商标**:TDF 商标政策允许"substantially unmodified"构建沿用 LibreOffice 名——我们只做打包/隔离 profile,不改动其代码,落在容忍范围;**不要**改它的品牌或深度魔改。
- 包体积 ~350 MB,安装器从 ~280 MB 涨到 ~630 MB(内嵌方案)。

**选项**:
a) **可选下载 pack(推荐)** — 安装器保持苗条;应用内"引擎页"提供"补齐 Office 转换"按钮,从官方 release URL 拉取带哈希校验的 LO pack(与现有 starter 同一登记/哈希/激活机制),断网可跳过。需在 PRIVACY.md 披露这一次联网行为。
b) 内嵌进安装器 — 开箱即用最强,体积 +350 MB。
c) 两者(安装器内嵌 + 可选下载给便携用户) — 打包与测试成本最高。

**拍板**:___

## DECISION-4 — OCR / Tesseract(E-11)

**现状**:OCR 管线代码就绪、验收标准已定义;Windows 用户开箱无引擎。

**事实**(2026-09-07 核查):
- Tesseract 引擎 **Apache-2.0**;官方 `tessdata` 仓库(含 `eng`、`chi_sim`,fast 变体)**全部 Apache-2.0**,可自由再分发。无专利池问题。
- 体积:引擎 ~5 MB + eng ~4 MB + chi_sim ~2 MB(fast 变体),合计 ~10 MB 级,体积可忽略。

**选项**:
a) **入第五个 starter pack(eng + chi_sim,推荐)** — 风险最低的引擎决策,法律上与 Poppler 无差别;OCR 是扫描件刚需,验收标准(pdftotext 非空 + 抽样命中)已备好。
b) 维持 host 发现 + 文档指引 — 零打包工作,但"扫描件转可搜索 PDF"对多数用户等于不存在。

**拍板**:___(若 a,确认语言集:eng + chi_sim / 加 chi_tra)

## DECISION-5 — 文件夹右键动词清单(E-05 已上线部分)

已注册 2 个 Directory 动词:**Convert folder to JPG / Convert folder to WebP**。要不要加 PNG(图片第三格式)或 PDF(整夹 PDF→PNG)?维持 2 条可防菜单膨胀。

**拍板**:___(维持 2 条 / 加 PNG / 其他)

---

## 一键回复格式(示例)

> DECISION-1 按推荐;DECISION-2 a;DECISION-3 a;DECISION-4 a(eng+chi_sim);DECISION-5 维持;提交。

每拍一项,工程侧当轮即可开工对应 E-xx(全部无额外等待)。

## 来源

- [tesseract-ocr/tessdata(全仓 Apache-2.0,含 chi_sim)](https://github.com/tesseract-ocr/tessdata)
- [Arch Linux tesseract-data-chi_sim(Apache)](https://archlinux.org/packages/extra/any/tesseract-data-chi_sim/)
- [LibreOffice Licenses(MPL-2.0)](https://www.libreoffice.org/licenses/)
- [TDF/Policies/TradeMark Policy(substantially unmodified 容忍)](https://wiki.documentfoundation.org/TDF/Policies/Trademark_Policy)
- [MPL 2.0 FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/)
- 证书部分见 `CODE_SIGNING_DECISION_BRIEF.md` 的来源列表。
