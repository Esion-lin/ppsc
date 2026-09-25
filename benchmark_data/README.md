# benchmark_data

本目录存放论文 §4（Implementation and Benchmark）的测量数据，共六个 CSV。除
`conversion_accuracy.csv` 为数值模拟外，其余数值均为 criterion 报告的**中位数**
（`median point_estimate`），非占位或估算。

共同参数：

- 委员会规模 `n ∈ {4, 8, 16, 32, 64}`，门限 `t = ⌊(n-1)/2⌋`（诚实多数，`t < n/2`）。
- 网络三档（`MemoryMailbox` 逐消息模拟）：
  - LAN：RTT 0.5 ms / 带宽 1 Gbps
  - MAN：RTT 20 ms / 带宽 100 Mbps
  - WAN：RTT 100 ms / 带宽 10 Mbps
- MPC 字段 `ark_bn254::Fr`；布尔分享 `F2 = GF(2^8)`。
- BFV：明文模数 `p = 65537`，`batch = 8`，`mult_depth = 2`。

---

## 1. online_latency_ms.csv — 协议微基准在线延迟（RQ1/RQ2）

- 对应论文图 `fig:online-latency`。
- 单位：**毫秒 (ms)**，在线墙钟延迟（协议开始 → 所有目标方接受输出）。
- 列：`n` + 12 列，即 4 个操作 × 3 档网络（`{lan,man,wan}_{handoff,mult,h2s,s2h}`）。

| 列 | 含义 |
|---|---|
| `*_handoff` | 跨委员会状态交接（Basic Handoff，dealer 重构 masked difference） |
| `*_mult` | 乘法 + 度降低 + 交接（`Π_mult`） |
| `*_h2s` | FHE 密文 → SS（门限密钥 Shamir 解密 + 逐模数 Lagrange 组合） |
| `*_s2h` | SS → FHE 密文（重构明文 + BFV 重加密） |

要点：`handoff`/`mult` 提交全部 `n` 方 masked share，延迟由 RTT 主导；`h2s`/`s2h` 参与方为
`t+1 ≈ n/2`，其 partial 通信量随 `n` 增长，故曲线随 `n` 上升（尤其 WAN 受带宽限制）。

## 2. e2e_latency_ms.csv — 端到端 workload 延迟（RQ3）

- 对应论文 RQ3，单位：**毫秒 (ms)**，单次合约调用的完整延迟。
- 列：`n` + 9 列，即 3 个 workload × 3 档网络（`{lan,man,wan}_{transfer,auction,analytics}`）。

| 列 | workload | 计算内容 |
|---|---|---|
| `*_transfer` | private transfer | BFV 门限 H2S → MPC 比较 + 条件转移 → BFV S2H |
| `*_auction` | sealed-bid selection | 4 个投标的私有最大值（3 次比较 + 条件选择） |
| `*_analytics` | batched analytics | 8 个值局部求和 + 1 次阈值比较 |

MPC 比较用 **32-bit bounded `Π_S2B`**（余额/投标值域 u32，交互从 254 位降到 33 位）。
WAN 下逐位交互（每 bit 一次「打开」= RTT）主导延迟，故 auction（3 次比较）在 WAN 约 10.9 s。

## 3. e2e_throughput_per_sec.csv — 端到端吞吐（RQ3）

- 对应论文图 `fig:e2e-throughput`。
- 单位：**invocations/s**，由 `1 / e2e_latency_ms` 换算（**单并发**，非并发稳态吞吐）。
- 列：与 `e2e_latency_ms.csv` 相同（3 workload × 3 网络）。

> 注意：此吞吐为单进程单并发下的 `1/latency`；论文图中已注明测量条件为
> 「single-concurrency (1/latency)」。真实的固定并发稳态吞吐需真实多节点并发测量，
> 属后续工作。

## 4. handoff_scaling.csv — 委员会轮换扩展性（RQ4）

- 对应论文图 `fig:handoff-scaling`，固定 `n=16, t=7`。
- `m ∈ {1,16,64,256,1024}` 个 live SS 记录**打包**成一次提交 + 一次广播（RTT 摊销）。
- 列：`m` + 7 列。

| 列 | 含义 |
|---|---|
| `lan_ms` / `man_ms` / `wan_ms` | 端到端旋转延迟（ms） |
| `traffic_mib` | 聚合通信量（MiB）= `m × 32 × (n+1)` 字节 |
| `*_per_record_ms` | 摊销延迟 = 总延迟 / `m` |

要点：RTT 在 m 个记录间摊销，故 `*_per_record_ms` 随 `m` 递减（本地 `O(m·t²)` 分摊）。

## 5. hybrid_ablation.csv — 混合消融（RQ5，第一部分）

- 对应论文表 `tab:hybrid-ablation`，固定 `LAN, n=16, t=7`，单位 **ms**。
- 列：`placement` + 3 列（transfer / selection / analytics）。

| placement | 含义 |
|---|---|
| `all_ss` | 所有操作在 SS/MPC（纯 MPC） |
| `all_fhe` | 所有 eligible 操作在 FHE；比较在 BFV 不可行 → `N/A` |
| `hybrid` | 算术 FHE + 比较 MPC（transfer 含 H2S/S2H；selection/analytics 含 H2S 输入） |

> All-FHE 的本地算术（BFV `EvalAdd`）约 0.13 ms，但比较（余额充足性/argmax/阈值）
> 在 BFV 不可行，故 workload 标 N/A——这正是 hybrid 的必要性所在。

## 6. conversion_accuracy.csv — 转换精度 sweep（RQ5，第二部分）

- 对应论文表 `tab:conversion-accuracy`。
- **数值模拟**（非 OpenFHE 实测），按论文 §3 的 truncation-and-wrap 误差模型
  （Eq. h2s/s2h effective-error），均匀归一化噪声，`100000` trials。
- 列：`noise_delta`（`|e|/Δ`）、`rho_delta`（`ρ/Δ`）、`trials`、`h2s_pct`、`s2h_pct`
  （mismatch 百分比）。

要点：当 `noise + ρ < Δ/2` 时错误率为 0（与论文精确条件一致）；`noise/Δ` 增大 → 错误率上升。

## 7. seed_generation.csv — 多委员会 seed 生成（真实 PRSS）

- 真实 PRSS `Π^{t,t}_ResharePair`（holder-set 构造）的 seed 生成 benchmark，诚实多数
  `t = ⌊(n-1)/2⌋`。
- 列：`n`、`t`、`holder_sets`（每委员会 holder 组数 = `C(n,t)`）、`seed_count`（总 seed 数
  = `C(n,t)²`）、`seed_per_party`（每方持有 seed 数 = `C(n-1,t)`）、`time_ms`（wall-clock）、
  `total_bytes`（seed 存储量 = `seed_count × 32 B`）。

要点：组合爆炸 `seed_count = C(n,t)²`；n=14/t=6 已 900 万 seed / 4.9 s / 288 MB，再大不可行，
说明 `reshare_pair_dealer`（`O(n·t)`）作为 benchmark 规避手段的动机。

## 8. sortition_seed.csv — 候选池大小 vs seed 成本

- 证明「候选池大小不影响 seed 生成成本」（导师「100 个候选节点很容易」）。
- 列：`pool_size`（候选节点池）、`committee_size`（sortition 选出的 committee 人数）、
  `threshold`、`sortition_ms`（sortition 抽样时间）、`seed_gen_ms`（真实 PRSS seed 生成）、
  `seed_count`。

要点：sortition 是 `O(committee)` 的随机抽样（微秒级、可忽略）；seed 生成只随 committee 的
`C(n,t)` 增长，与候选池大小无关（100 → 100,000 池子，seed 生成 ~0.47/0.88 ms 不变）。

## 9. seed_generation_extended.csv — 三组 seed 生成试验

- 三组试验，覆盖不同阈值策略与操作：
  - `fixed_t2_handoff`：固定 `t=2` 的 handoff `Π^{t,t}`（seed = `C(n,2)²`，多项式 n⁴）
  - `honest_majority_handoff`：诚实多数 `t=⌊(n-1)/2⌋` 的 handoff `Π^{t,t}`（seed = `C(n,t)²`，指数爆炸）
  - `honest_majority_mult`：诚实多数的乘法 `Π^{2t,t}`（源侧 degree-2t，seed = `n·C(n,t)`）
- 列：`experiment`、`n`、`t`、`src_degree`、`dst_degree`、`holder_sets_src/dst`、
  `seed_count`、`seed_per_party_src/dst`、`time_ms`、`total_bytes`。

要点：诚实多数 handoff 指数爆炸（n=13 已 294 万 seed / 3.2 s / 94 MB）；而乘法 `Π^{2t,t}`
因源侧 `holder_sets=n`，seed 成本比 handoff 低 ~132 倍（n=13：2.2 万 vs 294 万）。

---

## 数据来源与复现

- 原始采样：`D:\ppsc\target\criterion/**/new/raw.csv`。
- 复现命令（环境见 `D:\ppsc\docs\benchmark_guide.md`）：

```powershell
# 在线延迟（handoff/mult/H2S/S2H）
cargo bench -p ppsc-node --bench handoff_network
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench conversion_network

# 端到端 workload（RQ3）
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench workloads_e2e

# 轮换扩展性（RQ4）
cargo bench -p ppsc-node --bench handoff_scaling

# 消融（RQ5 第一部分）
cargo bench -p ppsc-node --bench hybrid_ablation
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench hybrid_ablation_fhe

# 转换精度（RQ5 第二部分，数值模拟）
cargo run -p ppsc-crypto --bin conversion_accuracy
```
