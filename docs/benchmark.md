# PPSC Benchmark 报告

## 1. 概述

本报告给出 PPSC（Privacy-Preserving Smart Contracts Engine）协议层与 OpenFHE 转换层的
基准测试数据，作为论文 §4（Implementation and Benchmark）的实现基线。覆盖导师文档
第五节「Benchmark 的正确方法」的三类：

- **A. Protocol microbenchmark**：Basic Handoff、Multiplication、条件转移、
  认证（handoff / multiply / verify_mac）、预处理（reshare_pair / rand_share）、
  OpenFHE 加解密与序列化。
- **B. Batch/vector benchmark**：B = 1/10/100/1000。
- **C. End-to-end benchmark**：单次 confidential transfer 延迟（Round 3 的三档
  网络 / 并发 TPS 因需网络模拟与真实链，见 §4.4 待办）。

## 2. 环境与配置

| 项 | 值 |
|---|---|
| 机器 | 12th Gen Intel(R) Core(TM) i7-12700H，14 核 / 20 线程，2.3 GHz |
| 操作系统 | Windows 11 家庭版 (10.0.26200) |
| Rust | rustc 1.94.1 |
| Git commit | `eac73f31aad704bd294a647c1e6a0326c839e95b` |
| OpenFHE | 本地 `openfhe-development`（MinGW64 静态库，BFVRNS） |
| 基准开始时间 | 2026-08-18 |
| 随机种子 | `StdRng::seed_from_u64(0)`（固定，可复现） |

**协议参数**：

- MPC 字段：`ark_bn254::Fr`（254-bit 素数域）；布尔分享：`F2 = GF(2^8)`。
- BFV：明文模数 `p = 65537`，`batch = 8`，`mult_depth = 2`。
- 委员会：handoff / auth 用 `t = 2`（n = 5/20/100）；multiply / auth-multiply 用
  `t = 1`（n = 3/20）；`conditional_transfer` / `verify_mac` 用 `t = 2`（n = 5/20）。
- 网络 benchmark（§3.7）：`n ∈ {4,8,16,32,64}`，门限 `t = ⌊(n-1)/2⌋`（诚实多数，
  满足论文 `t < n/2`）。
- PRSS：`reshare_pair` / `rand_share` 生成相关掩码与随机分享。

**方法论**：criterion 0.5.1（release profile，`-O2`）；每个 bench 预热后采集 100 个样本，
报告 `[low, mid, high]` 置信区间；正确性由 `cargo test` 单独保证（Round 1 冒烟），
不混入基准。

## 3. 结果

### 3.1 协议微基准（total latency）

| 协议 | n | 延迟（中位数） |
|---|---|---|
| `basic_handoff` | 5 | **51.9 µs** |
| `basic_handoff` | 20 | **1.20 ms** |
| `basic_handoff` | 100 | **35.7 ms** |
| `multiply_and_handoff` | 3 | **17.4 µs** |
| `multiply_and_handoff` | 20 | **1.24 ms** |
| `conditional_transfer`（S2B 254-bit） | 5 | **19.8 ms** |
| `auth_handoff` | 5 | **78.4 µs** |
| `auth_handoff` | 20 | **1.87 ms** |
| `auth_multiply` | 3 | **24.1 µs** |
| `auth_multiply` | 20 | **1.87 ms** |
| `verify_mac` | 5 | **117 µs** |
| `verify_mac` | 20 | **2.85 ms** |

### 3.2 分阶段 / per-party 与 dealer 计算时间（basic_handoff，n=20, t=2）

| 阶段 | 角色 | 延迟（中位数） |
|---|---|---|
| `source_masked_diff`（`[δ]_i = [x]_i − [r]_i`） | 每个源方本地 | **440 ns** |
| `dealer_reconstruct`（3 份 Lagrange） | dealer | **12.2 µs** |
| `destination_apply`（`[x]'_j = [r]'_j + δ`） | 每个目标方本地 | **222 ns** |

> 说明：分阶段总和 ≈ 13 µs，远小于 `basic_handoff` 整体（1.20 ms）。原因是当前
> `basic_handoff` 的 `reconstruct` 传入**全部 n 份**做 Lagrange（O(n²)）；真实部署只
> 收集最少 `t+1` 份即可，降到 O(t²)（≈ 12 µs）。见 §4.1。

### 3.3 预处理（preprocessing）

| 操作 | n | 延迟（中位数） |
|---|---|---|
| `reshare_pair`（相关掩码） | 5 | **290 µs** |
| `reshare_pair` | 20 | **42.4 ms** |
| `reshare_pair` | 100 | **≈ 27 s**（一次性预处理，见 §4.2） |
| `rand_share`（随机分享） | 5 | **137 µs** |
| `rand_share` | 20 | **15.8 ms** |

### 3.4 Batch/vector（`basic_handoff`，n=20, t=2）

| B | 总耗时 | 每值均摊 | values/s |
|---|---|---|---|
| 1 | 1.20 ms | 1.20 ms | **833 /s** |
| 10 | 12.1 ms | 1.21 ms | **826 /s** |
| 100 | 122 ms | 1.22 ms | **820 /s** |
| 1000 | 1.08 s | 1.08 ms | **926 /s** |

通信量（协议静态属性，见 §4.3）：单值 handoff 发送 `n × 40 + 32` 字节
（`n` 份 masked share 各 40 B + dealer 广播 32 B）。

### 3.5 OpenFHE 转换与序列化（BFV，batch=8）

| 操作 | 延迟（中位数） |
|---|---|
| `bfv_encrypt` | **13.5 ms** |
| `bfv_decrypt`（Multiparty 加法解密） | **11.6 ms** |
| `h2s`（Multiparty 解密 + Fr 分享） | **12.2 ms** |
| `s2h`（Fr 重构 + BFV 加密） | **13.8 ms** |
| `bfv_serialize`（密文序列化） | **1.62 ms** |
| `bfv_deserialize`（密文反序列化） | **12.5 ms** |

### 3.6 端到端（C 的简化版）

| 场景 | 延迟（中位数） |
|---|---|
| `confidential_transfer_single`（BFV 解密 → MPC 比较+条件转移 → BFV 重加密） | **59.4 ms** |

### 3.7 网络延迟（三档网络 LAN/MAN/WAN，真实多节点 RTT 模拟）

在线协议（handoff / mult）与转换（H2S / S2H）的多节点消息流经带 RTT + 带宽的
`MemoryMailbox` 模拟，测得**真实**的端到端在线延迟。RTT/带宽：LAN 0.5 ms / 1 Gbps、
MAN 20 ms / 100 Mbps、WAN 100 ms / 10 Mbps（论文须最终固定并披露）。

委员会规模 `n ∈ {4,8,16,32,64}`，门限 `t = ⌊(n-1)/2⌋`（诚实多数）。跨委员会的相关掩码
`([r]^{t1,comm1}, [r]^{t2,comm2})` 由**可信 dealer** 的 Shamir split 生成
（`reshare_pair_dealer`，`O(n·t)`），以避开 PRSS holder-set 构造在大 t 下的
`O(C(n,t)²)` 指数成本；在线协议只消费 mask、不依赖其生成方式，故在线延迟不受影响。
PRSS 版 `reshare_pair` 仍保留，用于小 t 的正确性验证。

| 操作 | n | LAN (ms) | MAN (ms) | WAN (ms) |
|---|---|---|---|---|
| handoff | 4 | 1.19 | 20.93 | 100.80 |
| handoff | 8 | 1.35 | 21.03 | 100.99 |
| handoff | 16 | 1.90 | 21.48 | 101.45 |
| handoff | 32 | 4.21 | 23.88 | 103.92 |
| handoff | 64 | 15.91 | 34.07 | 114.18 |
| mult | 4 | 1.20 | 20.83 | 100.83 |
| mult | 8 | 1.37 | 21.00 | 100.99 |
| mult | 16 | 1.92 | 21.43 | 101.54 |
| mult | 32 | 4.22 | 23.94 | 103.90 |
| mult | 64 | 14.51 | 33.89 | 114.15 |
| H2S | 4 | 31.94 | 89.05 | 601.46 |
| H2S | 8 | 56.41 | 113.07 | 625.54 |
| H2S | 16 | 106.66 | 159.28 | 674.01 |
| H2S | 32 | 205.51 | 254.08 | 770.11 |
| H2S | 64 | 400.61 | 458.76 | 968.22 |
| S2H | 4 | 13.14 | 23.01 | 63.62 |
| S2H | 8 | 13.37 | 23.25 | 63.94 |
| S2H | 16 | 14.08 | 23.89 | 64.91 |
| S2H | 32 | 18.38 | 27.84 | 68.77 |
| S2H | 64 | 37.23 | 47.05 | 88.04 |

> **门限策略**：`t = ⌊(n-1)/2⌋`（诚实多数，满足论文 `t < n/2` 且 `n > 2t`）。H2S 用
> **密钥 Shamir 门限（t-of-n）**：`t+1` 方各算一个 BFV partial（完整密文），dealer 逐模数
> Lagrange 组合 + fusion；因此 partial 数量与通信量随 `t+1 ≈ n/2` 增长，H2S 延迟随 n 明显上升
> （LAN 31.9→400.6 ms；WAN 下 10 Mbps 带宽支配 partial 传输，601→968 ms）。S2H 同理
> （`t+1` 方发 `Fr` share + dealer 重构 + BFV 加密）。handoff/mult 提交全部 n 份 masked share、
> dealer 广播到 n 方，延迟由 RTT 主导，几乎不随 t 变化。

### 3.8 端到端 workload（RQ3，per-invocation latency → throughput）

三个合约 workload 在**网络模拟**（`MemoryMailbox` RTT + 带宽）下、按委员会规模
`n ∈ {4,8,16,32,64}`、门限 `t = ⌊(n-1)/2⌋` 测得 per-invocation 延迟。MPC 比较用
**32-bit bounded `Π_S2B`**（余额/投标值域 u32，`bit_len + 1 = 33` 位），网络交互由
`ppsc-node::comparison_runtime` 逐「打开」路由（每次打开 = 提交 + 广播 = RTT）。
吞吐 = 单并发 `1/latency`（invocations/s）。

| Workload | 计算内容 |
|---|---|
| private transfer | BFV 门限 H2S → MPC 比较 + 条件转移 → BFV S2H |
| sealed-bid selection（auction, 4 bids） | 3 次 bounded 比较 + 条件选择（argmax） |
| batched analytics（8 值求和 + 阈值） | 局部求和 + 1 次 bounded 比较 |

**per-invocation 延迟（中位数，ms）**：

| Workload | n | LAN | MAN | WAN |
|---|---|---|---|---|
| transfer | 4 | 88.9 | 905.4 | 4498 |
| transfer | 8 | 115.1 | 932.1 | 4522 |
| transfer | 16 | 164.9 | 983.0 | 4574 |
| transfer | 32 | 279.6 | 1092.8 | 4687 |
| transfer | 64 | 519.9 | 1338.4 | 4935 |
| auction | 4 | 124.1 | 2249 | 10890 |
| auction | 8 | 132.6 | 2251 | 10893 |
| auction | 16 | 130.3 | 2260 | 10900 |
| auction | 32 | 151.8 | 2284 | 10925 |
| auction | 64 | 254.7 | 2376 | 11170 |
| analytics | 4 | 39.8 | 708.0 | 3430 |
| analytics | 8 | 41.6 | 708.7 | 3429 |
| analytics | 16 | 46.1 | 710.4 | 3429 |
| analytics | 32 | 50.8 | 715.5 | 3434 |
| analytics | 64 | 68.9 | 737.7 | 3459 |

**吞吐（单并发，1/latency，invocations/s）**：完整矩阵见
`e2e_throughput_per_sec.csv`（LAN/MAN/WAN × n）。量级：LAN transfer 11.2→1.9、
auction 8.1→3.9、analytics 25.1→14.5；MAN 0.4–1.4；WAN 0.09–0.29。

> **观察**：MPC 比较的逐位交互主导延迟——auction（3 次比较）在 WAN 下约 10.9 s/次，
> 因为每次 `Π_S2B` 是 `33` 位 ripple-carry，每位一次「打开」（= RTT）。32-bit（而非
> 254-bit）已使比较交互从 254 降到 33 位；进一步降低需 packed/并行 bit 分解或 batch 比较。

### 3.9 RQ4 / RQ5（导师新增）

**RQ4 委员会轮换扩展性**（`node/benches/handoff_scaling.rs`，n=16, t=7）：

`m ∈ {1,16,64,256,1024}` 个 live SS 记录**打包**成一次提交 + 一次广播（RTT 摊销），
测总旋转延迟 + 流量 + 摊销。中位数（ms）：

| m | LAN | MAN | WAN | Traffic (MiB) | LAN/record |
|---|---|---|---|---|---|
| 1 | 1.36 | 20.94 | 100.95 | 0.0005 | 1.36 |
| 16 | 3.17 | 22.88 | 103.64 | 0.0083 | 0.198 |
| 64 | 9.10 | 28.96 | 112.02 | 0.0332 | 0.142 |
| 256 | 32.92 | 53.43 | 145.22 | 0.1328 | 0.129 |
| 1024 | 127.66 | 152.11 | 278.33 | 0.5313 | 0.125 |

数据见 `benchmark_data/handoff_scaling.csv`。摊销（per-record）随 m 递减：RTT 摊销 +
本地 `O(m·t²)` 分摊。

**RQ5 消融**（LAN, n=16, t=7，中位数 ms）：

| Placement | Transfer | Selection | Analytics |
|---|---|---|---|
| All SS（纯 MPC） | 47.99 | 133.68 | 41.77 |
| All FHE | N/A（比较不可行） | N/A | N/A（阈值不可行） |
| Hybrid | 164.85 | 241.88 | 148.91 |

- All-SS 见 `node/benches/hybrid_ablation.rs`；Hybrid transfer 见 `workloads_e2e.rs`；
  Hybrid selection/analytics（H2S 输入）见 `fhe/benches/hybrid_ablation_fhe.rs`。
- All-FHE 的算术（BFV `EvalAdd`）本地约 **0.13 ms**，但比较在 BFV 不可行 → workload 标 N/A。
- Hybrid transfer 比 All-SS 慢（164.85 vs 47.99）：多出 H2S（门限解密）+ S2H（重加密）的 BFV 成本；
  但 All-FHE 无法完成比较，体现 hybrid 的必要性。

**RQ5 转换精度**（数值模拟，`crypto/src/bin/conversion_accuracy.rs`）：

按论文 §3.2 的 truncation-and-wrap 误差模型（Eq. h2s/s2h effective-error）数值模拟
`100000` trials，均匀归一化噪声，错误率（mismatch %）：

| Noise/Δ | ρ/Δ | H2S (%) | S2H (%) |
|---|---|---|---|
| 0.05 | 0.00 | 0.000 | 0.000 |
| 0.20 | 0.10 | 0.000 | 0.000 |
| 0.40 | 0.15 | 3.070 | 3.098 |
| 0.55 | 0.20 | 15.991 | 15.756 |

符合论文的精确条件：`|e_tot|+ρ < Δ/2` 时错误率 0。

### 3.10 多委员会 seed 生成（P1，真实 PRSS `Π_ResharePair`）

导师要求「5-6 人委员会真实跑一次 seed 生成，记录 seed 数、时间、通信/存储、组合爆炸」。
用**真实 PRSS**（`reshare_pair`，holder-set 构造）测 `Π^{t,t}_ResharePair`，诚实多数
`t=⌊(n-1)/2⌋`，记录 holder set 数、seed 数、每 party seed 数、wall-clock 与存储量：

| n | t | holder_sets | seed_count | seed/party | time_ms | bytes |
|---|---|---|---|---|---|---|
| 5 | 2 | 10 | 100 | 6 | 0.36 | 3.2 KB |
| 6 | 2 | 15 | 225 | 10 | 0.68 | 7.2 KB |
| 7 | 3 | 35 | 1,225 | 20 | 3.11 | 39 KB |
| 8 | 3 | 56 | 3,136 | 35 | 9.38 | 100 KB |
| 9 | 4 | 126 | 15,876 | 70 | 23.61 | 508 KB |
| 10 | 4 | 210 | 44,100 | 126 | 52.34 | 1.4 MB |
| 12 | 5 | 792 | 627,264 | 462 | 446.38 | 20 MB |
| 14 | 6 | 3,003 | 9,018,009 | 1,716 | 4,877.21 | 288 MB |

数据见 `benchmark_data/seed_generation.csv`。**组合爆炸**：`holder_sets = C(n,t)`，
`seed_count = C(n,t)²`，每 party 存 `C(n-1,t)` 个 seed。n=14/t=6 已 900 万 seed、4.9 s、
288 MB；再往上（n=16/t=7 → C(16,7)=11440 → 1.3 亿 seed）不可行——这正是 `reshare_pair_dealer`
（O(n·t)）作为 benchmark 规避手段的动机，但真实部署需 PRSS 的 holder-set 构造或更低成本生成。

**候选池大小不影响 seed 成本**（导师「100 个候选节点很容易」）：从候选池 sortition 选
5–6 人，seed 生成只取决于 committee 的 `n/t`，与候选池大小无关。数据见
`benchmark_data/sortition_seed.csv`：

| 候选池 | committee | sortition (ms) | seed 生成 (ms) | seed 数 |
|---|---|---|---|---|
| 100 | 5 (t=2) | 0.0003 | 0.47 | 100 |
| 100 | 6 (t=2) | 0.0002 | 0.88 | 225 |
| 1,000 | 5/6 | ~0.0002 | 0.47 / 0.95 | 100 / 225 |
| 10,000 | 5/6 | ~0.0002 | 0.45 / 0.88 | 100 / 225 |
| 100,000 | 5/6 | ~0.0003 | 0.52 / 0.93 | 100 / 225 |

sortition 是 `O(committee)` 的随机抽样（微秒级、可忽略）；seed 生成随 committee 的
`C(n,t)` 增长、与候选池无关。

**扩展试验**（三组，`benchmark_data/seed_generation_extended.csv`）：

| 试验 | 操作 | seed 数 | 结论 |
|---|---|---|---|
| 固定 t=2 | handoff `Π^{t,t}` | `C(n,2)²` | 多项式 n⁴，n=64 已 406 万 seed / 6.5 s / 130 MB |
| 诚实多数 t=⌊(n-1)/2⌋ | handoff `Π^{t,t}` | `C(n,t)²` | 指数爆炸，n=13 已 294 万 / 3.2 s / 94 MB |
| 诚实多数 | mult `Π^{2t,t}` | `n · C(n,t)` | 源侧 degree-2t 使 holder_sets=n，比 handoff 温和 ~132 倍 |

关键发现：multiplication 的 `Π^{2t,t}` 源侧 `holder_sets = C(n, n-2t)`，诚实多数下
`2t=n-1` → `holder_sets=n`、每 party 源侧只属 1 个 holder set，故 mult 的 seed 成本远低于
handoff（n=13：2.2 万 vs 294 万 seed）。

## 4. 评估

### 4.1 复杂度

1. **handoff / multiply / 认证是线性的（O(n)）**（协议本体逐 party）；`basic_handoff`
   的整体增长（n=5→20→100：52 µs→1.2 ms→35.7 ms）主要来自**重构开销**：当前
   `reconstruct` 传全部 n 份做 Lagrange（O(n²)），n=100 时约 35 ms。**只取 t+1 份可
   降到 O(t²)（≈ 12 µs），是最大优化点之一**。
2. **MPC 比较是 e2e 的瓶颈**：`Π_S2B` 逐位 ripple-carry，每位一次交互（打开）。已实现
   **32-bit bounded 比较**（`greater_than_or_equal_bounded` + `s2b_bounded`，值域 u32），
   把交互从 254 位降到 33 位；网络化后（`comparison_runtime`）WAN 下单次比较 ≈ 34×RTT
   ≈ 3.4 s，仍主导 transfer/auction/analytics 的 WAN 延迟。进一步需 packed/并行 bit 分解。
3. **预处理成本高于在线协议**：`reshare_pair` n=20 已 42 ms（在线 handoff 1.2 ms），
   n=100 约 27 s——相关掩码生成是 PRSS 固有复杂度，需批量/并行预处理。
4. **认证倍增**：`auth_handoff` ≈ 1.6× `basic_handoff`（value+MAC 两条分享）；
   `verify_mac` 最贵（2.85 ms，含两次度降低乘法）。
5. **OpenFHE 是数量级瓶颈（无网络）**：单次 BFV 操作 12–14 ms，转换（H2S/S2H）与其相当。
   网络化后（§3.7/§3.8），WAN 下 MPC 比较的逐位 RTT 交互成为主导（transfer ~4.5 s/次），
   BFV 操作退居次要。

### 4.2 协议静态属性（rounds / messages / bytes）

以 `basic_handoff`（n 方，t 门限）为例：

| 指标 | 值 |
|---|---|
| number of rounds | 2（提交 masked share → 广播 delta） |
| messages sent | `n`（源方提交）+ 1（dealer 广播）= **n+1** |
| total bytes | `n × 40 + 32`（每份 share 40 B + delta 32 B） |
| per-party bytes（源方） | 40 B 发送 + 32 B 接收（delta 广播） |
| per-party bytes（目标方） | 32 B 接收 |

`conditional_transfer`（32-bit bounded）：在线打开次数 `1 + 33`（`Π_S2B`）+ `2×2`
（两次条件选择）≈ 38 次；每次打开 2 轮（提交 + 广播），消息 `38 × n` 级，字节
`38 × n × 40` 级。全宽度（254-bit）版本为 `1 + 254 + 4 ≈ 259` 次打开。

### 4.3 与论文的一致性 / 差异

- 论文 §4 目前为空；本报告数据可作为投稿 §4 基线。
- 已知简化（记入「与论文不一致清单」）：
  1. H2S/S2H 掩码 `r` 用 PRF 小整数（论文 §3.2 随机字段元素）；
  2. S2B 用 `GF(2^8)`（论文 `F2` 记号，等价）；
  3. OpenFHE 为 RNS 多模数（论文假设单 `Q = q_c`），门限解密已用逐模数分享实现；
  4. H2S 用 Multiparty/门限解密后 dealer 可见明文（论文 Fig 4 用 mask `r`，dealer
     只见 `δ = x − r`；该 mask 步骤未接入，为简化项）。
  5. 网络 benchmark 的跨委员会掩码由可信 dealer 生成（`reshare_pair_dealer`，`O(n·t)`），
     非论文 Fig 2 的 PRSS holder-set 构造（后者在大 t 下 `O(C(n,t)²)` 不可行）；在线协议
     不受影响，PRSS 版保留用于正确性验证。
  6. e2e 的 MPC 比较用 **32-bit bounded `Π_S2B`**（论文附录 `Π_S2B` 是全宽度 `⌈log2 q⌉` 位）：
     `greater_than_or_equal_bounded` 把 `a − b + 2^32` 分解成 33 位，值域声明为 u32（余额/投标），
     交互从 254 降到 33 位；正确性由 centered lift 保证（`|a−b|<2^32≪q/2`，无 `mod q` wrap）。

### 4.4 局限性 / 待办（导师要求未覆盖）

| 项 | 状态 |
|---|---|
| Round 1 正确性（n=3, batch=1/10） | ✅ `cargo test` 覆盖 |
| Round 2 稳定数据（≥20-30 次） | ✅ criterion 100 samples |
| 三档网络 handoff / mult 在线延迟 | ✅ `run_*_with_latency`（真实 RTT 模拟） |
| 三档网络 H2S / S2H 在线延迟 | ✅ 真实（门限 H2S + S2H，`MemoryMailbox` RTT+带宽） |
| 每次实验保存机器配置 / git / 版本 / 参数 / 种子 / 原始 CSV | ✅ `benchmark.md` + `target/criterion/**/new/raw.csv` |
| 起止时间 | ⚠️ 记录开始时间，结束时间未逐一记录 |
| peak memory | ❌ 未测（Windows 上需额外工具） |
| **Round 3 端到端三 workload（transfer/auction/analytics）** | ✅ 三 workload × 三档网络 × n∈{4,8,16,32,64} 的 per-invocation latency 已测（32-bit bounded 比较 + 网络化）；吞吐 = 单并发 1/latency（`e2e_throughput_per_sec.csv`） |
| RQ4 委员会轮换扩展性（m live SS records） | ✅ 批量 handoff 网络化 bench，`handoff_scaling.csv` |
| RQ5 转换精度（噪声/wrap 错误率） | ✅ 数值模拟（`conversion_accuracy` bin），填 `tab:conversion-accuracy` |
| RQ5 消融（All-SS/All-FHE/Hybrid） | ✅ All-SS + Hybrid 实测；All-FHE 比较标 N/A（BFV 无比较），算术 EvalAdd ≈ 0.13 ms |
| sustained throughput（并发稳态 + 真实分布式多节点） | ❌ 未做：当前为单进程单并发 `1/latency`；并发稳态吞吐需真实多节点 + 并发框架 |
| bytes/value（batch） | ✅ 已给静态字节公式，未做真实网络抓包 |

## 5. 复现

```powershell
# 环境
$env:TMP = "D:\tmp"; $env:TEMP = "D:\tmp"
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH

# 协议微基准 + batch（默认 target）
cargo bench -p ppsc-crypto --bench handoff
cargo bench -p ppsc-crypto --bench multiplication
cargo bench -p ppsc-crypto --bench comparison
cargo bench -p ppsc-crypto --bench auth
cargo bench -p ppsc-crypto --bench preprocessing

# OpenFHE 转换 / 序列化 / 端到端（GNU target）
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench conversion
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench transfer

# 端到端 workload（RQ3，三档网络 × n，32-bit 比较；GNU target）
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench workloads_e2e

# RQ4 轮换扩展性（默认 target）
cargo bench -p ppsc-node --bench handoff_scaling

# RQ5 消融：All-SS（默认 target）+ Hybrid selection/analytics 与 All-FHE 算术（GNU target）
cargo bench -p ppsc-node --bench hybrid_ablation
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench hybrid_ablation_fhe

# RQ5 转换精度（数值模拟）
cargo run -p ppsc-crypto --bin conversion_accuracy
```

HTML 报告输出到 `target/criterion/report/index.html`；原始采样在
`target/criterion/**/new/raw.csv`。
