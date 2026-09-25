# PPSC

PPSC 是一个密态智能合约研究原型。

```text
.ppsc 源码
  → ppsc-compiler 生成 manifest/operator DAG + Solidity Gateway
  → 将 Gateway 部署到 Anvil（构造函数向 ControlPlane 发布合约和函数哈希）
  → 用户调用 Gateway 的 createAccount/deposit/transfer/getBalance
  → ControlPlane 记录 execution，并指定 committee
  → 常驻 manifest_committee_daemon 执行编译出的 DAG
  → 节点 PostgreSQL 保存密态状态，daemon 将结果和变量新引用回写链上
  → 用户从 Gateway 取得授权 opening，或直接从链上查询密态变量的 dataId
```

## 从这里开始

请按 [编译、部署、调用、回写完整演示](docs/README-compiled-contract-e2e.md) 逐步运行。
该演示实际完成：

```text
Alice 创建密态账户
Bob 创建密态账户
Alice 密态入账 100
查询：Alice = 100
Alice 调用编译生成的 transfer，密态转给 Bob 30
查询：Alice = 70，Bob = 30
```

已验证的结果：三个授权 opening 分别解码为 `100`、`70`、`30`，对应 execution 状态均为 `6 (Completed)`。

## 编译的合约

主示例是 [ConfidentialToken.ppsc](examples/contracts/ConfidentialToken.ppsc)：

```text
privacy contract ConfidentialToken {
  private FheUint balance[address];

  function transfer(address to, FheUint amount) public {
    SecretBool sufficient = H2S(FHE.ge(balance[msg.sender], amount));
    require(sufficient);
    balance[msg.sender] = FHE.sub(balance[msg.sender], amount);
    balance[to] = FHE.add(balance[to], amount);
  }

  function getBalance() public {
    Sint balanceShare = H2S(balance[msg.sender]);
    return Pick(balanceShare);
  }
}
```

编译命令会同时生成 Runtime manifest 和可部署的 Solidity Gateway：

```bash
cargo run -p ppsc-compiler --bin ppsc -- build \
  examples/contracts/ConfidentialToken.ppsc \
  --sol-out contracts/src/generated
```

输出位于：

```text
target/ppsc/ConfidentialToken/manifest.json
target/ppsc/ConfidentialToken/operators.json
target/ppsc/ConfidentialToken/abi.json
target/ppsc/ConfidentialToken/hashes.env
contracts/src/generated/ConfidentialTokenGateway.sol
```

Gateway 不是摆设：部署时它调用 `publishContract/publishFunction`，用户调用它时又通过 `invokeCompiledFor` 在 ControlPlane 创建任务。daemon 会校验链上的 manifest hash 与本地编译产物完全一致后才执行。

## 当前实现状态

| 模块 | 状态 | 说明 |
|---|---|---|
| `.ppsc` 编译器 | 已接通 | 生成 manifest、operator DAG、ABI、哈希和 Solidity Gateway |
| 生成 Gateway | 已接通 | 可部署；提供 `createAccount/deposit/withdraw/transfer/getBalance` |
| ControlPlane | 已接通 | 合约/函数发布、任务队列、committee、密态变量引用、结果提交 |
| Manifest Runtime VM | 已接通 | 执行 FHE/MPC/H2S/S2H、`require`、状态写和 `Pick` |
| Committee daemon | 已接通本地演示 | 自动分配/启动任务、计算、签名、提交结果、更新变量和 opening |
| PostgreSQL | 已接通 | 每节点独立状态、认证输入、上传/结果 outbox、游标和 processed task |
| 动态 handoff | 已有协议与存储实现 | 旧/新 committee 数据交接路径已实现；主编译合约演示使用单节点 committee |
| 密码 backend 桥接 | 已接通 | daemon 可保持长期 crypto 子进程，逐个执行 manifest 的 MPC/FHE 操作 |
| 用户上传 | 已接通 | 用户签名、过期时间、nonce 防重放、PostgreSQL outbox、daemon 自动链上登记 |
| 真实 OpenFHE | 已接 daemon | BFV、Shamir、H2S/S2H、比较已适配；本机仍需单独安装 OpenFHE 原生库 |
| 生产安全 | 未就绪 | 未审计；本地 verifier、固定密钥、开发 opening 均不得用于生产 |

## 余额查询的两种含义

- 查询“密态余额在哪里”：直接读 Gateway 的 `balanceVariable(user)` 和 ControlPlane 的 `stateVariables(...)`，不触发 Runtime；返回的是当前 `dataId`、表示类型、slot 和版本，不泄露余额。
- 查询“余额明文是多少”：必须由账户本人调用 `getBalance` 请求授权 opening，committee 执行 `H2S → Pick` 后将结果加密给用户。当前本地演示为了可观察性把开发 opening 写入 Gateway；生产版本必须改为用户公钥加密结果。

## 其他演示

- [明文 ERC-20 → 密态余额 → 密态转账 → 提款](docs/README-complete-confidential-demo.md)：真实 ERC-20 托管和 gas escrow 路径。
- [密态合约语言](docs/privacy-contract-language.md)：类型、语法和 operator 映射。
- [密态合约协议](docs/confidential-contract-protocol.md)：链上事实、链下秘密、sortition 与 handoff。
- [Runtime 接口](docs/runtime.md)：MPC/FHE/转换接口与开发 backend。
- [存储和 handoff](docs/storage-handoff-workflow.md)：数据位置与委员会轮换。

## 验证

```bash
cargo test -p ppsc-compiler -p ppsc-runtime
TEST_DATABASE_URL='postgres://127.0.0.1/postgres' \
  cargo test -p ppsc-runtime --test manifest_postgres -- --test-threads=1
cargo clippy -p ppsc-runtime --all-targets -- -D warnings
forge test -vv
```

当前验证覆盖编译器、Runtime、长期 crypto 进程、认证上传摘要、PostgreSQL 上传 outbox 和 Solidity 合约。真实 OpenFHE binary 只有在本机提供 `openfhe-development` 头文件/静态库后才能链接。

## 安全边界

- `PlaintextManifestBackend` 仅供本地联调，payload 不是密码学安全密文。
- `LocalAcceptAllSortitionVerifier` 只用于 Anvil；测试网使用 `EcdsaSortitionVerifier`，生产应接 VRF/random beacon verifier。
- 主演示使用 threshold=1 的单节点 committee，以验证编译—部署—调用—回写集成；不能代表生产阈值安全。
- 生产 `Pick` 必须验证授权并只返回用户公钥加密结果。
- 合约、密码协议和 handoff 实现尚未经过生产安全审计。
