# DECISION-1 决策简报：Authenticode 代码签名证书

**文档类型：** 负责人拍板材料（spec `EXPERIENCE_SPEC_PLAN.md` DECISION-1）
**日期：** 2026-09-07（行情核查日）
**状态：** 等待 Leo 拍板。拍板后 E-01 签名流、E-02 Win11 现代菜单、E-12 用户研究的签名包全部解锁；CI 侧已就绪（`release-candidate.yml` 的签名步骤会在 secret 配好后自动启用，无需改代码）。

---

## 一、2026 年的关键事实（先读这个，旧经验已过时）

1. **EV 不再保证立即过 SmartScreen。** Microsoft 官方文档与 DigiCert 公告都已确认：EV 签名的应用仍可能弹警告，直到 Microsoft 积累足够下载遥测。社区实测（ToDesktop、SSL.com）认为 OV 与 EV 在 SmartScreen 起始信誉上现已基本等价，EV 最多"更快起效"（数天 vs 数周）。
2. **OV 与 EV 的私钥都强制硬件介质**（CA/Browser Forum 2023-06 起要求：USB token 或云 HSM）。**"PFX 文件直接放 CI secret"在合规 CA 已买不到**——这对 CI 集成方式有直接影响（见下文§三）。
3. **2026-02 起证书最长有效期缩短为 1 年**，多年付折扣正在消失——按年预算，不要按多年锁价。
4. EV 的**仍然不可替代**用途：Windows 内核驱动签名（本项目不需要）。
5. 本项目是 Apache-2.0 开源项目、个人开发者主体（无公司邓白氏码的话 OV/EV 的组织验证走个人/个体户路径，流程更长、可选 CA 更少）。

## 二、价格行情（2026-09 快照，逐年波动 ±20%）

| 档位 | 年价（约） | 说明 |
|---|---|---|
| OV（SSL.com、Comodo 等） | $96–$290/年 | SSL.com 约 $96–129/年最便宜档 |
| EV（CodeSignCert、Sectigo、DigiCert） | $296–$996/年 | DigiCert EV 高达 ~$996/年 |
| 开源开发者档（Certum 等，若以开源项目身份申请） | 约 €50–100/年 | 需要逐家核对当前条款，简报未逐一验证 |

## 三、私钥存放 × CI 集成（这是真正要选的东西）

| 方案 | 拍板后 CI 要做的事 | 风险/成本 |
|---|---|---|
| **A. CA 云签名 + KSP 中间件**（SSL.com eSigner、Certum SimplySign）——推荐 | 在 runner 上装 CA 的 KSP/CSP 中间件（一个安装步骤 + 凭据 secret），`signtool` 命令不变。现有骨架已预留安装步骤位置 | 私钥永不离云 HSM（合规、可吊销）；云服务年费可能单算；依赖 CA 的中间件质量 |
| B. USB token 物理邮寄 | 每次 CI 用不了；只能本地手动签名（`target\sign-test.bat` 路线），release 流程退回手工 | 最便宜；发布节奏受制于人在哪、token 在哪；与本仓库 CI 绿纪律相悖 |
| C. 自管 PFX + CI secret | 仅当选择的 CA 仍发非合规 PFX（多为代理转售旧库存，不建议） | 不合规库存随时被吊销；SmartScreen 信誉绑定 CA 主体，CA 倒了信誉清零 |

**工程推荐：方案 A + OV 证书（SSL.com OV + eSigner，或 Certum SimplySign 开源档）**，年预算按 **$130–300** 报。理由：本项目无驱动签名需求；SmartScreen 差异已被 2026 年现实抹平；省下的钱与等待时间直接换成 E-12 用户研究和更多格式 pack。EV 的额外 ~$300/年买不到可验证的收益。

## 四、拍板需要回答的四个问题（复选即可）

1. 类型：**OV**（推荐）/ EV
2. 供应商：SSL.com / Certum（开源档）/ Sectigo / 其他
3. 预算上限：$___ /年
4. 私钥路线：**方案 A（云签名 KSP）**（推荐）/ B（token 手动）/ C（PFX，不推荐）

## 五、拍板后的执行清单（工程已备好，预计 1 个工作日内完成）

1. 按 CA 流程完成主体验证与证书签发（OV 个人/组织验证通常 1–3 天）。
2. 将云签名凭据写入 repo secrets（沿用 updater 密钥纪律：只经 `printf | gh secret set`，绝不经 cmd shim）。
3. 手动触发 `release-candidate.yml` 验证签名步骤激活：三件产物 `Get-AuthenticodeSignature` 为 `Valid`、带时间戳、`SHA256SUMS` 在签名后重生成。
4. 干净 VM 双击安装器验证 SmartScreen（OV 预期：初期可能仍有提示，随下载量消退——发布说明如实标注）。
5. 解锁 E-02（MSIX Publisher 需与证书主体一致）与 E-12。

## 来源

- [Microsoft Learn – SmartScreen reputation for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
- [ToDesktop PSA – EV certs do not grant immediate reputation anymore](https://www.todesktop.com/blog/posts/windows-apps-psa-ev-certs-do-not-grant-immediate-reputation-anymore)
- [DigiCert KB – EV-signed application showing SmartScreen warnings](https://knowledge.digicert.com/alerts/ev-signed-application-showing-microsoft-defender-smartscreen-warnings)
- [SSL.com – Which Code Signing Certificate do I Need? EV or OV?](https://www.ssl.com/faqs/which-code-signing-certificate-do-i-need-ev-ov/)
- [SSL.com – OV Code Signing](https://www.ssl.com/products/software-integrity/code-signing/ov/)
- [RapidSSLonline – DigiCert EV pricing & 2026 term changes](https://www.rapidsslonline.com/ssl-brands/digicert/ev-code-signing.aspx)
