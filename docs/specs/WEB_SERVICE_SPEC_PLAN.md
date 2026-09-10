# Anole Web Service Spec Plan（浏览器在线版）

状态：Draft — 等 Leo 拍板 2 个决策点后执行
日期：2026-09-10
负责人拍板记录：Leo 选定"免费/便宜平台 + 完整功能"（2026-09-10 对话）。

## 一句话增量陈述

浏览器打开就能用的 Anole：用户上传文件、选目标格式、拿走转换结果与验证回执 —— 服务器端跑真实的 anole-server 与转换引擎，与本地桌面版并存（不是替代）。

## 定位与不变量

- **并存策略**：桌面版继续是隐私卖点与开源门面（本地零上传）；Web 版服务"不想安装"的用户。两者共用同一个 core/server 代码与引擎 pack 机制。
- **验证回执不变**：Web 版每次转换同样返回机器可读的验收报告 —— 这是与 CloudConvert/howtoconvert.co 的差异化。
- **开源免费**：Web 版不收费（与既有定价决策一致），成本靠便宜平台消化。

## 平台选型（2026-09 核实过）

| 平台 | 费用 | 规格 | 完整引擎集(含 LibreOffice+Tesseract) | 备注 |
|---|---|---|---|---|
| **Render free** | $0 | 512MB RAM / 0.1 CPU / ephemeral 磁盘 / 15min 休眠 | ❌ 只够 Lite 引擎集 | 冷启动 ~30-60s；SQLite 每次重启清零 |
| **HF Spaces PRO** | ~$9/月 | 16GB RAM / 2 vCPU / 50GB ephemeral / 48h 休眠 | ✅ 富余 | Docker 免费层已锁，PRO 解锁；定位偏 demo 但可公开访问 |
| Railway / Fly.io | ~$5-10/月 usage | 512MB-1GB 档 | ⚠️ 2GB 档才稳 | Railway 免费层仅 $1/月额度 |
| Oracle Cloud free ARM | $0 永久 | 4 核 24GB VM | ✅ | 要信用卡注册+自运维（安全/证书/重启），运维负担在 Leo |

**推荐：两档走** —— 先 Render free 上 Lite 版（$0 验证有没有人用），Docker 镜像同一套；需要完整功能时把同一镜像部署到 HF PRO 或 Railway（改个部署目标，不改代码）。

## 架构

```
浏览器 ── Anole Web SPA (React, /apps/web)
              │ REST (现有 anole-server 路由 + 新增 upload/download)
              ▼
        anole-server (容器内, SQLite 队列)
              │ 外部进程
              ▼
        引擎 pack（Lite: poppler+ffmpeg+pandoc ≈250MB / Full: +LibreOffice+Tesseract ≈1.2GB）
```

1. **`apps/web`（新增）**：React SPA，复用桌面版设计语言（暖黄/chicago95 风格、变色龙品牌）与 desktopModel 的纯逻辑层（目标推荐、Plan 大白话渲染 —— 该层是纯 TS，可直接复用）。不引入 Tauri API。
2. **`anole-server` 扩展**（现有 REST API 已全路由 e2e 验证过）：
   - 托管 SPA 静态资源（同一容器、同源，免 CORS）
   - 上传端点：multipart，大小上限、扩展名白名单（对齐支持矩阵）
   - 临时文件生命周期：转换完成即删输出副本，保留报告元数据；输入 TTL（默认 1 小时硬删）
   - 并发/限流：同时转换数上限（Lite=1，Full=2）、每 IP 排队
   - 健康检查端点（平台探活防休眠选项）
3. **Dockerfile（新增）**：多阶段 —— rust 构建层（缓存 deps）+ 前端构建层 + 运行层（apt 装精简引擎或复用 Linux pack 解包机制）。Lite/Full 用同一 Dockerfile 的 build-arg 切换。
4. **部署物**：`render.yaml`（free 起步）+ 手册级部署文档。

## 与桌面版的功能对齐（"完整功能"清单）

| 功能 | 桌面版 | Web 版方案 |
|---|---|---|
| 单文件转换+回执 | ✅ | W1 |
| 目标格式推荐/Plan 预览 | ✅ | W1（复用 desktopModel 纯逻辑） |
| 拖放/多文件批量 | ✅ | W2（浏览器 File API） |
| 持久队列+进度 | ✅ | W2（SQLite；free 档接受重启丢失，付费档挂卷持久） |
| 预设 | ✅ | W2（匿名本地存 localStorage；账号体系不在范围） |
| Explorer 右键/暗色/多语言 | ✅ | 暗色 W2；右键不适用；中英 W2 |
| 引擎诊断页 | ✅ | W3（透明展示服务器引擎集与状态） |

## 分期

- **W1 — 可用（预计 1-2 天）**：Dockerfile(Lite) + server 静态托管/上传/TTL + 极简 SPA（上传→选格式→Plan→转换→下载+回执）+ 部署 Render free + 真跑验收。
- **W2 — 完整功能（2-3 天）**：队列/进度/历史页、批量、预设、暗色、i18n、Full 引擎集 build-arg + 付费档部署演练（HF PRO 或 Railway，Leo 开账号）。
- **W3 — 打磨（按需）**：限流熔断（日配额+CPU 熔断）、滥用防护、usage 页、自定义域名（anole.xxx 由 Leo 提供）、监控告警。

## 隐私与安全承诺（公开写明）

- 上传文件仅用于本次转换，TTL 后自动删除；不用于训练、不分发。
- 无账号、无跟踪（合规最小日志：时间/大小/格式，不含文件名）。
- 大小上限（默认 50MB，Render free 磁盘约束）、单 IP 并发 1、转换超时（默认 120s）。
- 风险登记：公开免费服务可能被脚本滥用（W3 的配额是缓解）；转换受版权保护文件的 DMCA 风险低但存在（保留封禁手段）。

## 待 Leo 拍板（2 个）

1. **DECISION-W1**：先 $0 Lite 起步再升档（推荐），还是直接开 ~$9/月 HF PRO/Railway 上完整版？
2. **DECISION-W2**：上传上限默认 50MB 是否合适（影响等待体验与滥用面）？

## 非目标

- 不做用户账号/付费墙（与开源免费决策一致）。
- 不做 WASM 浏览器端转换（外部引擎架构不可行，已论证）。
- 不替换桌面版；macOS 原生打包是另一条独立 wave。
