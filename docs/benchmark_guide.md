# PPSC Benchmark 环境与实现指南

本文档说明 PPSC（Privacy-Preserving Smart Contracts Engine）的 benchmark 如何运行、依赖的环境与前置条件，以及论文中各协议在 Rust 代码中的对应实现。目标是让读者能在一台新机器上复现 `docs/benchmark.md` 与论文 §4 的数据。

> 约定：工作目录为 `D:\ppsc`（9 个 crate 的 workspace）。本文所有路径与命令均基于当前开发机（Windows 11 + MSYS2 MinGW64）。

---

## 1. 前置条件

### 1.1 硬件与系统（当前开发机）

| 项 | 值 |
|---|---|
| CPU | 12th Gen Intel Core i7-12700H，14 核 / 20 线程，2.3 GHz |
| 内存 | 未在数据中记录（benchmark 未测 peak memory） |
| 操作系统 | Windows 11 家庭版（10.0.26200） |

### 1.2 软件工具链

| 项 | 版本 / 路径 | 说明 |
|---|---|---|
| Rust | rustc 1.94.1（workspace `rust-version = "1.75"`） | 编译全部 Rust crate |
| MSYS2 MinGW64 g++ | `C:\msys64\mingw64\bin\g++.exe` | 编译 OpenFHE C ABI（`capi.cpp`）与链接 |
| GNU target | `x86_64-pc-windows-gnu` | `ppsc-fhe` 必须用此 target（见 §2.2） |
| OpenFHE | `D:\ppsc\openfhe-development`（源码 + 已编译静态库） | BFVRNS 方案 |

### 1.3 OpenFHE 静态库

`ppsc-fhe` 的 `build.rs` 编译 `cpp/capi.cpp` 并链接以下预编译静态库（位于 `openfhe-development/build/lib/`）：

```
libOPENFHEpke_static.a
libOPENFHEcore_static.a
libOPENFHEbinfhe_static.a
```

同时链接 `stdc++`、`winpthread`，并显式追加 `ucrtbase` 与 `moldname`（`build.rs` 中以 `-Wl,-Bstatic,-lucrtbase,-Bdynamic` 等传入，用于满足 `_fileno/_setmode` 的引用顺序）。

`build.rs` 按顺序探测 g++ 路径：`mingw64` → `ucrt64` → `clang64`，找不到时回退到 `g++`。

### 1.4 Cargo 配置（`.cargo/config.toml`）

```toml
[http]
proxy = ""                 # 关闭代理，直连 rsproxy

[target.x86_64-pc-windows-gnu]
linker = "g++"             # OpenFHE 由 MinGW64 编译，用 g++ 链接以拉入 CRT/C++ 运行时

[source.crates-io]
replace-with = "rsproxy"
[source.rsproxy]
registry = "sparse+https://rsproxy.cn/index/"
```

### 1.5 环境变量

每次运行 `cargo`（尤其涉及 `ppsc-fhe`）前需设置：

```powershell
$env:TMP = "D:\tmp"
$env:TEMP = "D:\tmp"
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
```

`TMP/TEMP` 指向 `D:\tmp` 是因为 OpenFHE 的 C++ 编译会产生大量临时文件；`PATH` 加 MinGW64 使 g++/链接器可被 `cc` crate 找到。

---

## 2. 环境搭建步骤

1. **安装 Rust**（≥1.75，推荐 1.94.x）。
2. **安装 MSYS2 MinGW64**：`pacman -S mingw-w64-x86_64-gcc`（含 g++）。
3. **添加 GNU target**：`rustup target add x86_64-pc-windows-gnu`。
4. **编译 OpenFHE**：在 `D:\ppsc\openfhe-development` 下用 CMake + MinGW64 编译，产出 `build/lib/` 中的三个静态库。`build.rs` 期望的 include 目录为：
   - `src/core/include`、`src/pke/include`、`src/binfhe/include`
   - `third-party/cereal/include`、`build/src/core`
5. **配置 crate 源**：按 §1.4 写 `.cargo/config.toml`（或确保能访问 rsproxy.cn）。
6. **建临时目录**：确保 `D:\tmp` 存在。
7. **验证**：`cargo check --target x86_64-pc-windows-gnu -p ppsc-fhe` 通过即环境就绪。

---

## 3. Workspace 结构（9 crates）

| crate | 职责 |
|---|---|
| `ppsc-core` | 基础类型：`NodeId`、`CommitteeId`、`ExecutionId`、`DataId`、`Commitment`、`PublicBytes`、`SecretBytes` |
| `ppsc-crypto` | 论文 MPC 协议层（纯数学，无网络）：Shamir、PRSS、handoff、认证、比较、S2B、转换、打包、字段抽象 |
| `ppsc-protocol` | `HandoffSession` 状态机 + 消息类型（`MaskedShareSubmission`/`MaskedDeltaBroadcast`）+ 错误 |
| `ppsc-network` | `MemoryMailbox`（进程内邮箱，RTT/带宽模拟）+ `NetworkProfile` |
| `ppsc-storage` | `MessageRepository` / `TaskRepository` trait |
| `ppsc-chain` | `ChainGateway` trait（链上协调抽象） |
| `ppsc-node` | 节点编排层：`handoff_runtime`（多节点 handoff 闭环）、`comparison_runtime`（网络化比较/条件转移） |
| `ppsc-runtime` | 可插拔 MPC/FHE backend 的合约 runtime（`BalanceRuntime` + `PlaintextBackend`） |
| `ppsc-fhe` | OpenFHE C ABI + BFV 门限密钥分享 + 网络化 H2S/S2H |

依赖方向（自下而上）：`core` → `crypto`/`protocol`/`network`/`storage`/`chain` → `node` → `runtime`；`fhe` 依赖 `core`/`crypto`/`network`。

Workspace 级 lint（`Cargo.toml`）：`unsafe_code = deny`、`dbg_macro = deny`、`unwrap_used = deny`、`todo = warn`。

---

## 4. 论文协议 → 代码实现映射

### 4.1 字段与分享（论文 §2 Preliminaries）

| 论文概念 | 代码 | 位置 |
|---|---|---|
| `ShareField` 抽象 | `pub trait ShareField` | `crypto/src/mpc/field.rs` |
| 算术域 `F_Q`（素数域） | `ark_bn254::Fr`（254-bit，BLS12-381 标量域），经 blanket `impl ShareField for F: PrimeField` | `field.rs` |
| 布尔域 `F_2` | `F2 = GF(2^8)`（AES 多项式 `x^8+x^4+x^3+x+1`；特征 2 下加法=XOR，仅 AND 需交互） | `field.rs` |
| 度-`t` Shamir 分享 `[x]^{⟨t,Q⟩}` | `Committee<F>`、`ShamirShare<F>`、`split`/`reconstruct`、`evaluation_points`、`holder_basis`/`holder_sets`/`combinations` | `crypto/src/mpc/shamir.rs` |

设计要点：`ShamirShare` 故意不实现 `Debug`/`Display`/`Clone`（防止秘密进入日志/被复制）；`Committee::new(t, n)` 仅校验 `n > t`（乘法协议额外要求 `n > 2t`）。

### 4.2 预处理 PRSS（论文 §3.2 Shared Offline Preprocessing）

| 协议 | 函数 | 位置 |
|---|---|---|
| `Π^t_RandShare` | `rand_share` / `rand_share_small` | `crypto/src/mpc/prss.rs` |
| `Π^{t1,t2}_ResharePair` | `reshare_pair` / `reshare_pair_small` | `crypto/src/mpc/prss.rs` |
| 可信 dealer 变体（benchmark 用） | `reshare_pair_dealer` | `crypto/src/mpc/prss.rs` |
| PRF | `PrssSeed`（HMAC-SHA256，`derive_element`/`derive_small`） | `crypto/src/mpc/prss.rs` |

关键点：
- PRSS 的 holder-set 构造 `holder_sets(n, d) = {A : |A| = n−d}`，规模 `C(n, d)`；`reshare_pair` 对 `H_1 × H_2` 双重循环，复杂度 `O(C(n,t1)·C(n,t2))`。当 `t ≈ n/2` 时指数爆炸（n=32 已 `C(32,15)≈5.6e8`）。
- 因此 benchmark 的跨委员会掩码用 **`reshare_pair_dealer`**：dealer 采样 `r` 后对两侧做 Shamir `split`，`O(n·t)`，代数上等价、但「无单方知道 r」的性质由 PRSS 版保留。在线协议只消费 mask，不依赖其生成方式。

### 4.3 交接与乘法（论文 §3.1 Basic Handoff / Case-1）

| 协议 | 函数 | 位置 |
|---|---|---|
| `Π_Handoff`（纯数学） | `basic_handoff` + `masked_difference_shares` + `apply_difference` | `crypto/src/mpc/handoff.rs` |
| `Π_mult`（度降低 + 交接） | `multiply_and_handoff` | `crypto/src/mpc/handoff.rs` |
| 多节点网络闭环 | `run_handoff` / `run_handoff_with_latency` / `run_mult_with_latency` | `node/src/handoff_runtime.rs` |
| 状态机 + 消息 | `HandoffSession`、`MaskedShareSubmission`、`MaskedDeltaBroadcast`、`HandoffError` | `protocol/src/handoff.rs` |

流程（dealer 中介）：源方本地算 `[δ]_i = [x]_i − [r]_i` → 提交给 dealer → dealer Lagrange 重构 `δ = x − r` → 广播 `δ` → 目标方本地 `[x]'_i = [r]'_i + δ`。`run_*_with_latency` 用 `MemoryMailbox` 逐消息注入 RTT。

### 4.4 认证（论文 §3.1 Case-3 + 附录）

| 协议 | 函数 | 位置 |
|---|---|---|
| 认证分享 `⟨⟨x⟩⟩` | `AuthenticatedSharing` | `crypto/src/mpc/auth.rs` |
| 认证线性运算 | `auth_add` / `auth_scalar_mul` | `auth.rs` |
| 认证交接 | `authenticated_handoff` | `auth.rs` |
| 认证乘法 | `authenticated_multiply` | `auth.rs` |
| MAC 校验 | `verify_mac` | `auth.rs` |

MAC 关系：`γ_x = α·x`，`α` 为共享 MAC 密钥；认证交接/乘法在下一轮 `verify_mac` 中打开 `([z]−[xα])·[r]` 校验是否为零（deferred authentication）。

### 4.5 比较与 S2B（论文 §3.2 / 附录 Π_S2B）

| 协议 | 函数 | 位置 |
|---|---|---|
| 全宽度 `Π_GEQ`（`a−b` 的符号位） | `greater_than_or_equal` | `crypto/src/mpc/comparison.rs` |
| 32-bit bounded `Π_GEQ` | `greater_than_or_equal_bounded` | `comparison.rs` |
| 全宽度 `Π_S2B`（`⌈log2 q⌉` 位） | `s2b` | `crypto/src/mpc/s2b.rs` |
| bounded `Π_S2B`（`bit_len` 位） | `s2b_bounded` + `generate_bit_extract_pair_bounded` | `s2b.rs` |
| 位分解辅助 | `ripple_add`/`full_adder`/`secret_and`/`centered_lift`/`two_complement_bits` | `s2b.rs` |
| B2A | `b2a` + `generate_random_bit` | `comparison.rs` |
| 条件选择 | `conditional_select` | `comparison.rs` |
| 条件转移 | `conditional_transfer_strictly_greater` | `comparison.rs` |
| 网络化比较/条件转移 | `comparison_runtime`（`geq_bounded_network`、`conditional_transfer_network`、`conditional_select_network`、`open_shares`、`BALANCE_BITS=32`） | `node/src/comparison_runtime.rs` |

32-bit bounded 比较的正确性：`w = a − b + 2^32 ∈ [0, 2^33)`（因 `a,b < 2^32 ≪ q/2`，无 mod-q wrap），`a ≥ b ⟺ w` 的第 32 位为 1。`s2b_bounded` 用 `centered_lift`（把 `δ = w − r mod q` 精确提升回 `(−q/2, q/2)` 的 `w−r`，因 `|w−r| < 2^33`）后做 33 位 ripple-carry 加法，交互从 254 位降到 33 位。

### 4.6 FHE↔SS 转换（论文 §3.1 Case-2）

| 协议 | 函数 | 位置 |
|---|---|---|
| 简化 LWE 版 H2S/S2H（测试用） | `h2s` / `s2h` / `h2s_mask` / `s2h_mask` / `encrypt` / `decrypt` | `crypto/src/mpc/conversion.rs` |
| 动态模数域（逐 RNS 模数 Shamir） | `DynField` / `DynShare` / `split` / `reconstruct` / `eval_points` | `fhe/src/dynamic.rs` |
| BFV 门限密钥分享 | `bfv_share_key_per_modulus` / `per_modulus_lambdas` | `fhe/src/bfv_shamir.rs` |
| BFV 明文分享 / 门限解密 / 重构加密 | `bfv_share_plaintext` / `bfv_share_plaintext_threshold` / `bfv_shares_to_ciphertext` | `fhe/src/bfv_shamir.rs` |
| 网络化 H2S / S2H | `h2s_threshold_with_network` / `s2h_with_network` | `fhe/src/network_conversion.rs` |
| OpenFHE C ABI | `fhe_bfv_partial` / `fhe_bfv_shamir_decrypt` / `fhe_bfv_shamir_fuse` / `make_secret_from_coeffs` 等 | `fhe/cpp/capi.cpp` |

关键点：
- BFV 用明文模数 `p = 65537` 存储加密余额；MPC 分享在 `Fr`（254-bit）上，避免 16-bit `q/2` 边界。
- OpenFHE 是 RNS 多模数（`mod_reduce_to_level0` 不减少 limb 数），故门限解密**逐模数**分享密钥（`DynField` 上 `t-of-n`）+ 逐模数 Lagrange 组合 `λ_j`，再 BFV fusion。H2S 只 `t+1` 方算 partial（`bfv_share_partial` = `c_0 + s_i·c_1`）。

### 4.7 打包分享（论文 §3.3 Packed）

| 协议 | 函数 | 位置 |
|---|---|---|
| 打包参数 | `PackedParams`（`D = t+L−1`，`n > 2D`） | `crypto/src/mpc/packed.rs` |
| 打包分享 | `packed_split` | `packed.rs` |
| 打包 pair | `packed_pairgen` | `packed.rs` |
| 打包度降低 | `packed_degree_reduce` | `packed.rs` |

### 4.8 网络模拟

| 概念 | 实现 | 位置 |
|---|---|---|
| `NetworkProfile`（LAN/MAN/WAN） | `one_way_latency` / `rtt` / `bandwidth_bps` | `network/src/memory.rs` |
| `MemoryMailbox` | `with_latency` / `with_profile` / `send_with_size` / `drain` | `network/src/memory.rs` |

三档网络参数（论文须最终披露）：

| 档 | RTT | one-way | 带宽 |
|---|---|---|---|
| LAN | 0.5 ms | 250 µs | 1 Gbps |
| MAN | 20 ms | 10 ms | 100 Mbps |
| WAN | 100 ms | 50 ms | 10 Mbps |

---

## 5. Benchmark 如何运行

### 5.1 分类

| 类别 | bench 文件 | crate | target |
|---|---|---|---|
| 协议微基准（handoff/mult/比较/认证/预处理/负载） | `crypto/benches/{handoff,multiplication,comparison,auth,preprocessing,workloads}.rs` | `ppsc-crypto` | 默认 |
| 网络在线延迟（handoff/mult） | `node/benches/handoff_network.rs` | `ppsc-node` | 默认 |
| OpenFHE 转换/序列化 | `fhe/benches/{conversion,transfer}.rs` | `ppsc-fhe` | GNU |
| 网络化 H2S/S2H | `fhe/benches/conversion_network.rs` | `ppsc-fhe` | GNU |
| 端到端 workload（RQ3） | `fhe/benches/workloads_e2e.rs` | `ppsc-fhe` | GNU |

### 5.2 命令

```powershell
# 前置环境（每个 shell 都要）
$env:TMP = "D:\tmp"; $env:TEMP = "D:\tmp"
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH

# 协议微基准（默认 target）
cargo bench -p ppsc-crypto --bench handoff
cargo bench -p ppsc-crypto --bench multiplication
cargo bench -p ppsc-crypto --bench comparison
cargo bench -p ppsc-crypto --bench auth
cargo bench -p ppsc-crypto --bench preprocessing

# 网络在线延迟（handoff/mult × LAN/MAN/WAN × n）
cargo bench -p ppsc-node --bench handoff_network

# OpenFHE 转换 / 端到端（GNU target）
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench conversion
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench transfer
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench conversion_network
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench workloads_e2e
```

可用 `-- --sample-size N --warm-up-time S` 控制采样（criterion）；用 `-- <filter>` 只跑匹配子串的组（如 `-- lan`）。

### 5.3 测试与 lint

```powershell
# 全部非 FHE 测试
cargo test --workspace --exclude ppsc-fhe

# FHE 测试（GNU target；OpenFHE 静态库多线程有状态干扰，必须单线程）
cargo test --target x86_64-pc-windows-gnu -p ppsc-fhe -- --test-threads=1

# lint / 格式
cargo clippy --workspace --all-targets
cargo fmt --all --check
```

### 5.4 数据产出

- criterion 报告 HTML：`target/criterion/report/index.html`
- 原始采样：`target/criterion/**/new/raw.csv`；中位数：`.../new/estimates.json`（`median.point_estimate`，单位纳秒）
- 已整理的图表数据：论文目录 `benchmark_data/` 下三个 CSV（`online_latency_ms.csv`、`e2e_latency_ms.csv`、`e2e_throughput_per_sec.csv`），说明见该目录 `README.md`

---

## 6. 关键参数速查

| 参数 | 值 |
|---|---|
| 委员会规模 | `n ∈ {4,8,16,32,64}` |
| 门限 | `t = ⌊(n−1)/2⌋`（诚实多数，`t < n/2`） |
| MPC 算术域 | `ark_bn254::Fr`（254-bit） |
| 布尔分享 | `F2 = GF(2^8)` |
| BFV | 明文模数 `p = 65537`，`batch = 8`，`mult_depth = 2` |
| 比较值域 | 32-bit（`BALANCE_BITS = 32`，余额/投标 u32） |
| 随机种子 | `StdRng::seed_from_u64(0)`（可复现） |
| 交叉委员会掩码 | `reshare_pair_dealer`（O(n·t)，见 §4.2） |

---

## 7. 与论文的已知差异（实现层）

1. **MPC 模数**：论文设 `Q = q_c`（转换层单素数系数模数、不设明文模数 `p`）；实现用 `Fr`（254-bit）+ BFV `p = 65537`。
2. **H2S mask `r`**：论文 Fig h2s 中 dealer 只见 `δ = x − r`；实现里 BFV 门限解密后 dealer 直接见明文（`bfv_shamir_decrypt` 的 `masks` 参数已就位但未接入，因 OpenFHE 明文 scale 与 partial 的 noise/level 对齐问题）。
3. **S2B**：论文附录 `Π_S2B` 为全宽度 `⌈log2 q⌉` 位；实现额外提供 32-bit bounded 版用于比较。
4. **离线掩码**：benchmark 用可信 dealer（`reshare_pair_dealer`）替代 PRSS holder-set（见 §4.2）。
5. **e2e 吞吐**：为单进程单并发 `1/latency`，非真实多节点并发稳态吞吐（后续工作）。
