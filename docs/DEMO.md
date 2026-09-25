# 演示：入金（deposit）与交易（transfer）

给导师演示 PPSC 的「入金」和「交易」两条链路。全程**真密码学**：余额是 BFV 密文、
比较在 MPC 分享上秘密完成、解密靠门限密钥。

> 一条命令即可跑通入金 → 查询 → 交易 → 查询。

## 0. 准备（演示前跑一次，避免现场等编译）

```powershell
$env:TMP = "D:\tmp"; $env:TEMP = "D:\tmp"
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
cargo build --target x86_64-pc-windows-gnu -p ppsc-fhe --example real_confidential_demo
```

## 1. 入金（deposit）

**对应论文**：§2.2「FHE upload and registration」——用户的余额以 FHE 密文形式存到链下。

**命令**（demo 的前半段）：

```powershell
cargo run --target x86_64-pc-windows-gnu -p ppsc-fhe --example real_confidential_demo
```

**入金阶段的输出**：

```
== deposit (BFV encrypt balances) ==
sender ciphertext  : 1311953 bytes, first 24 bytes 0x0100000040...
receiver ciphertext: 1311953 bytes, first 24 bytes 0x0100000040...
```

**讲什么**：
- 入金 `100`（sender）和 `20`（receiver）并没有存成明文数字，而是变成了两个 **约 1.3 MB 的 BFV 密文**。
- 密钥不是单点的：它由 **Multiparty 门限密钥**（3 方）联合持有，单一节点拿不到明文。

## 2. 交易（transfer）

**对应论文**：§3.1「FHE↔SS conversion」+「MPC comparison」——`H2S → MPC(比较+条件转移) → S2H`。

**交易阶段的输出**：

```
== query before ==
sender  = 100   receiver= 20          ← 门限解密查询

== transfer ==
H2S: multiparty-decrypt + Shamir-share each balance
MPC: secret comparison sender(100) > minimum(50), then conditional sub/add
S2H: reconstruct sharing + BFV re-encrypt

== query after ==
sender  = 70    receiver= 50          ← 条件转移后
```

**讲什么（按三步）**：
1. **H2S（密文 → MPC）**：把密文余额门限解密后，立即做 **Shamir 秘密分享**，明文不出现。
2. **MPC 比较**：`sender(100) > minimum(50)` 的充足性检查，是在**分享上秘密比较**的（`Π_S2B` 位分解 + 条件选择），比较位全程不打开。
3. **S2H（MPC → 密文）**：转移结果 `70`/`50` 重构后**重新 BFV 加密**回密文。

**收尾**：最终 `sender=70, receiver=50`——说明「入金后转走 30」在密文/分享上正确完成。

## 3. 如果导师要「看明文被保护」

指出这两点即可：
- 密文是 1.3 MB 的真实 BFV 密文（`0x0100...` 开头，不是数字）；
- 比较结果 `100 > 50` 只体现在「余额从 100 变 70」，从未打印出比较位本身。

## 附：正确性 / 性能（可选）

```powershell
# 正确性（35 个测试）
cargo test --target x86_64-pc-windows-gnu -p ppsc-fhe -- --test-threads=1

# 论文 §4 性能数据
cargo bench --target x86_64-pc-windows-gnu -p ppsc-fhe --bench workloads_e2e
```

---

## 要主动说明的边界（别让导师误以为已分布式）

当前演示是**单进程模拟**：3 方 Multiparty 密钥、Shamir `t=2/n=5` 都在一个进程里跑。密文是真 BFV、比较是真 MPC，但「多节点 + 网络」还没接——真分布式在 `ppsc-node`（`run_handoff_with_latency` / `comparison_runtime`）那层，走 `MemoryMailbox`。
