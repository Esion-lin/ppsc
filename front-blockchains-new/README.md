# 区块链示范平台（Blockchain_sys）

基于 **Next.js 14（App Router）+ Tailwind CSS 3 + framer-motion** 的区块链示范平台门户，

## 快速开始

```bash
npm install
npm run dev        # 本地开发 http://localhost:3000
npm run build      # 生产构建（静态导出，○ Static）
npm run start      # 启动生产服务
```

## 课题一：PPSC 演示

访问 `/topic-one`，依次执行 `init → deploy → deposit → transfer → withdraw`。
`init` 构建 Rust 程序并启动独立 Anvil / PostgreSQL；`deploy` 从 `.ppsc` 编译并部署
Verifier、ControlPlane、ConfidentialTokenGateway，校验真实回执与 manifest hash，启动
`manifest_committee_daemon` 和 `manifest_crypto_service`，创建 Alice / Bob 账户。

输入由 `manifest_encrypt_input` 仅使用委员会公钥生成实际 BFV 密文，经用户签名上传后登记 dataId。
常驻 daemon 自动处理任务，OpenFHE 密钥在同一密码进程中保持；无明文后端回退。
入账 100、转账 30、Bob 出账 20 后，Alice / Bob 的实际账本余额为 70 / 10。
本例为编译合约密态记账，不包含 ERC20 储备或手续费兑换。
`status` 会通过 getBalance 取得开发 opening；该值公开写入本地链。显隐按钮仅控制界面显示。

第一次运行，在仓库根目录准备真实后端：

```bash
python3 scripts/build-real-backend.py
python3 scripts/test-real-crypto.py
```

依赖 Python 3.9+、C++17 编译器、git、Rust/Cargo、Foundry、PostgreSQL（initdb/pg_ctl）。
构建脚本下载固定 OpenFHE v1.5.1，静态编译到 `target/`，不安装系统库。
使用 `npm run dev -- --hostname 127.0.0.1` 启动前端。演示接口仅开放给本机 development 同源请求。
页面显示真实部署回执、daemon / 密码子进程 PID、公钥 SHA-256 指纹以及链上 dataId。
刷新页面保留会话；`stop` 关闭本次整组进程；重新 `init` 创建全新链和密钥。
密码进程退出后旧密文无法恢复，需重新初始化。临时目录保留供排查，不保存秘密密钥。

当前密码委员会仍在同一进程内运行，H2S/S2H 使用解密再分享或重加密。
链上单节点 threshold=1、accept-all verifier 和公开 opening 均为本地集成实现。

顶部「尝试PPSC」跳转到 `/wallet`。Wallet 和网页 demo 各自创建隔离环境，互不覆盖；
Wallet 也保留手工连接本机 8545 的方式。

无活动演示时，从仓库根目录运行 `python3 scripts/test-web-demo.py`，验证三笔部署回执、
实际密码交易、余额与状态根、常驻进程 / 公钥稳定性、重复提交拦截、日志脱敏与停止清理。

## Wallet：本地链密态账户

访问 `http://127.0.0.1:3000/wallet`，或从门户顶部的「尝试PPSC」进入。
本页面只连接本机 **Anvil / Chain ID 31337**，使用本地解锁测试账户，不需要钱包扩展。

原生依赖按上一节构建后，概览的「一键部署本地环境」自动完成：

- 在空闲本地端口启动独立 Anvil 与 PostgreSQL；
- 编译并部署默认 Verifier、ControlPlane、ConfidentialToken Gateway；
- 启动真实 OpenFHE/BFV + Shamir daemon 和密码子进程；
- 创建默认 Alice / Bob 密态账户，并自动填写 Gateway、连接钱包。

展示真实运行 PID、部署进度和日志。刷新页面恢复服务器会话；「停止本地环境」关闭本次全部进程。
重新启动创建新链和新密钥。交易按环境会话隔离，旧会话不回退到 8545。
若需手工运行，仍可使用[端到端文档](../docs/README-compiled-contract-e2e.md) 的 8545 流程。
前端使用独立终端启动：

```bash
cd front-blockchains-new
npm install
npm run dev -- --hostname 127.0.0.1
```

1. 在概览点击「一键部署本地环境」，等待就绪后自动连接；顶部可切换 Alice / Bob。
2. 默认 Gateway 自动配置；手工环境才需要在「连接配置」填写地址。
3. 默认 Alice / Bob 账户已创建；新部署合约或其他账户通过「设置账户」创建。
4. 在交易表单的「上传数据 · 获取 dataId」中输入整数金额，或选择用当前合约公钥生成的 `.bfv` 文件（1 KB–4 MB），点击「上传并获取 dataId」。服务端使用原生客户端加密、以当前本地测试账户签名上传，等待 daemon 完成链上登记后自动填入 dataId。
5. 「入金」填写该账户授权的 `dataId`；「转账」填写收款地址与金额输入 `dataId`；「出账」调用 Gateway 的 `withdraw`。
6. 在交易记录查看链上回执和 ControlPlane 执行进度。概览点击「显示余额」或「刷新余额」，实际调用 `getBalance` 并等待 daemon 返回 opening；「隐藏余额」控制页面显隐。余额状态引用变化后，旧数值隐藏并提示刷新。

上传与余额查询要求连接一键启动的受管环境，支持前十个 Anvil 测试账户。输入金额范围为 0–499122176；合约运算总量也须保持在 BFV 安全范围内。密文文件必须匹配当前合约公钥；普通 JSON、文本等不能直接作为金额密文上传。手工 8545 环境仍可通过原生 CLI 上传并填写 dataId。
**当前开发版余额 opening 会公开写入本地链；隐藏按钮不能撤回链上公开值。**

**入金/出账是该编译合约的密态记账语义，不是法币充值或 ETH 充值。**
输入由项目原生用户客户端仅使用委员会公钥加密，再由用户签名上传；浏览器不接触密码服务私钥。
受管环境使用真实 BFV / Shamir 后端；输入金额在本机服务端加密，上传、余额查询均使用固定的本地开发账户签名。
当前页适配 `ConfidentialTokenGateway`，不能自动为任意上传合约生成交互表单。

「加密合约」支持拖放 `.ppsc` 源文件（最大 256 KB），调用真实 PPSC 编译器并下载
`manifest.json`、`abi.json`、`operators.json`、`hashes.env` 和生成的 Gateway Solidity 源码。
须先在项目根目录运行 `cargo build -p ppsc-compiler --bin ppsc`。
上传文件在独立临时目录编译，响应后清理；页面中的产物刷新后不保留，请及时下载。
编译完成后点击「自动部署合约」，服务端使用保留的源码重新编译并核对 manifest hash，
部署独立 Verifier / ControlPlane / Gateway 并启动该合约的 daemon，不接收浏览器自定义 Solidity 或可执行命令。
编译产物部署凭据保留 1 小时、最多 16 份；过期后重新编译。同一凭据禁止重复部署。
兼容 balance / createAccount / deposit / withdraw / transfer / getBalance 接口的合约自动连接钱包；其他合约显示部署结果供 ABI 调用。
部署记录展示地址、交易哈希、确认区块、daemon PID、输入上传地址、公钥路径；既有合约的 daemon 保持运行。
自定义合约的账户不会自动初始化，可由「设置账户」或 Gateway 自己的初始化函数建立。

浏览器持久化当前网络的 Gateway 地址和最近 100 笔交易元数据；不持久化私钥或明文余额。数据任务结果仅保存在服务端进程内存，最多 100 份。
RPC 与编译接口仅在 development 模式接受本机同源请求；RPC 仅使用服务端创建的本地环境地址或固定 8545，
拒绝原生资产转账、任意 RPC 方法和外部网络。生产构建仅可预览页面，接口返回 403。

```bash
npm run test:wallet  # ABI 与 cast 对比、RPC 边界、真实上传编译、记录校验
npx tsc --noEmit
# 仓库根目录、没有活动钱包环境时：
python3 scripts/test-wallet-environment.py  # 一键环境、上传部署、独立 daemon、真实调用、停止清理
# 已启动钱包环境时：使用第十个测试账户，增加 12 个测试 PPSC，不停止环境
python3 scripts/test-wallet-data.py  # 金额/文件上传、链上登记、入金、实际余额、失败边界
# 可选：8545 空闲时启动临时 Anvil，真实部署并验证 Gateway 调用，结束后自动停止
WALLET_CHAIN_TEST=1 npm run test:wallet
```

测试依赖现有 Foundry `cast` 和 `target/debug/ppsc`。可用 `CAST=/absolute/path/to/cast` 指定 cast。
可选链上测试还需要 `anvil`、`forge build` 产生的合约产物及已有的 ConfidentialToken 编译产物。
数据联调测试覆盖实际 daemon 计算与余额 opening；若有第二份运行中的合约，也验证不同公钥的密文被拒绝。

## 主题配置

- 品牌主色：金棕色 `#bd7c40`（Tailwind `ms` 色板见 `tailwind.config.ts`）
- 深浅主题：CSS 变量定义在 `app/globals.css`（`:root` 浅色 / `[data-theme='dark']` 深色）
- 主题切换：`next-themes`（`app/providers.tsx`，默认深色、`storageKey=meta_theme`）
- 字体：优先使用本机 `Inter` / `Poppins`，回退到中文及系统字体；构建不依赖 Google Fonts

## 内容编辑

所有站点文案集中在 **`lib/content.ts`**：

| 字段 | 对应板块 |
|---|---|
| `siteTitle` / `siteSubtitle` | 站点标题 / 副标题 |
| `navItems` | 顶部导航（含下拉菜单层级） |
| `slides` | 顶部 Banner 轮播（3 张，`image` 留空则用品牌渐变） |
| `techTopics` | 核心技术板块（4 个，`layout` 控制左右分栏） |
| `caseItems` | 应用案例（3 个，`tags`/`showDetail`/`image` 可选） |

替换 `XX` 占位文案与 `image` 图片地址即可上线。

## 目录结构

```
app/
  layout.tsx        # 根布局（字体、metadata、Providers）
  providers.tsx     # next-themes ThemeProvider
  page.tsx          # 首页（导航 + Banner + 核心技术 + 应用案例 + 页脚）
  globals.css       # 主题变量 + Tailwind + 全局样式
components/
  Navbar.tsx        # 导航栏（桌面下拉 / 移动端菜单）
  HeroCarousel.tsx  # Banner 轮播
  CoreTech.tsx      # 核心技术左右分栏
  Cases.tsx         # 应用案例左右翻页轮播
  ThemeSwitcher.tsx # 深浅切换开关
  SectionHeading.tsx# 板块标题
lib/
  content.ts        # 站点内容配置
```
