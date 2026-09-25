# PPSC 密态合约语言（第一阶段）

本阶段把论文中的写法直接编译为一个与具体密码库解耦的 operator DAG。链上只接收 `bytes32 dataId`，密文本体仍由链下存储节点保管；runtime committee 按 DAG 调用现有 FHE、MPC 与 H2S/S2H 后端。

## 1. 合约写法

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
    Sint share = H2S(balance[msg.sender]);
    return Pick(share);
  }
}
```

完整示例见 `examples/contracts/ConfidentialToken.ppsc`。

## 2. 类型与数据位置

| 类型 | 表示 | 链上 ABI | 数据本体 |
|---|---|---|---|
| `address` / `uint` | 明文 | `address` / `uint256` | 链上或调用上下文 |
| `FheUint` | 同态密文 | `bytes32 dataId` | 链下存储节点 |
| `Sint` | 秘密分享 | `bytes32 dataId` | committee 各节点本地数据库 |
| `SecretBool` | 秘密分享布尔值 | 不直接公开 | committee 各节点本地数据库 |

`private FheUint balance[address]` 声明的是逻辑状态。用户不能指定余额变量地址；runtime 根据 `(contractId, stateName, key)` 解析当前 `dataId`。执行完成后，committee 把新密文存入节点，再由控制面原子更新新的 `dataId` 和存储节点集合。

## 3. 运算接口

- `FHE.encrypt(uint) -> FheUint`
- `FHE.add/sub(FheUint, FheUint) -> FheUint`
- `FHE.ge(FheUint, FheUint) -> FheBool`（仅编译器内部类型）
- `H2S(FheUint) -> Sint`
- `H2S(FheBool) -> SecretBool`
- `S2H(Sint) -> FheUint`
- `MPC.add/sub(Sint, Sint) -> Sint`
- `MPC.ge(Sint, Sint) -> SecretBool`
- `require(SecretBool)`：committee 联合判断，不把余额或 amount 打开
- `Pick(Sint)`：显式、受授权的 opening；查询结果应只加密给请求用户
- `receiveEncryptedToken` / `sendEncryptedToken`：swap/托管合约适配器调用点

FHE 和 SS 之间不会隐式转换。比如 `MPC.add(balance[msg.sender], amount)` 会编译失败，必须显式使用 `H2S`。

## 4. 编译

在仓库根目录运行：

```bash
cargo run -p ppsc-compiler --bin ppsc -- build \
  examples/contracts/ConfidentialToken.ppsc
```

预期输出：

```text
compiled examples/contracts/ConfidentialToken.ppsc
artifacts target/ppsc/ConfidentialToken
```

生成文件：

- `manifest.json`：状态、函数、selector、operator DAG、状态写集合和 opening 输出。
- `operators.json`：供 runtime 读取的逐函数 operator DAG。
- `abi.json`：链上入口 ABI；密态参数被编码为 `bytes32 dataId`。
- `hashes.env`：部署/调用工具可以直接读取的函数 selector。

## 5. 执行边界

编译器只描述计算，不自行解密。链上控制面收到函数调用后创建任务；sortition 选中的 committee 拉取对应 DAG 和 `dataId`，存储节点把密文/份额 handoff 给新 committee。committee 计算后提交结果承诺及新位置，控制面更新状态引用并从用户预付 gas 中补偿提交节点。

`ppsc-runtime::manifest` 已可以直接读取该 manifest，按 DAG 执行 FHE/MPC/H2S/S2H、原子提交状态写，并将存取款 hook 作为待提交动作返回。当前提供的 `PlaintextManifestBackend` 仅用于本地流程测试；生产 daemon 应实现 `ManifestCryptoBackend`，把 opaque payload 分发给真实 FHE/MPC committee。

manifest executor 已接入长期运行 daemon：daemon 从链上顺序任务索引恢复 selector、公开参数和私密 `dataId`，只在本节点属于当前 committee 且 execution 进入 `Running` 后执行。当前还需补齐 threshold 签名聚合与 result outbox 链上提交器。

## 6. 可恢复任务与节点数据库

控制面为每个 execution 持久保存 selector、私密参数的 `dataId[]`、ABI 编码的公开参数、requester、deadline 和状态，并提供顺序任务索引。`inputRoot` 同时承诺私密引用和公开参数，防止执行期间替换 receiver 等公开值。

每个计算节点使用独立 `DATABASE_URL`。`0002_manifest_runtime.sql` 保存：

- 该节点持有的合约密态状态；
- 由 `dataId` 定位的调用输入；
- 按控制合约和 node ID 隔离的下一任务游标；
- 已完成、跳过或拒绝的 execution 记录。

任务结果记录和游标推进处于同一 PostgreSQL 事务中，因此 daemon 重启后从上一个未完成任务继续，而不会静默跳过任务。

## 7. 启动 manifest committee daemon

每个节点使用自己的数据库和公开节点地址，不需要用户私钥：

```bash
export DATABASE_URL='postgres://ppsc:ppsc@localhost:5432/ppsc_node_1'
export RPC_URL='http://127.0.0.1:8545'
export CONTROL='0x...'
export CONTRACT_ID='0x...'
export NODE_ADDRESS='0x...'
export NODE_ID='node-1'
export MANIFEST_PATH='target/ppsc/ConfidentialToken/manifest.json'
export POLL_INTERVAL_SECONDS=2

cargo run -p ppsc-runtime --bin manifest_committee_daemon
```

daemon 按链上顺序索引持续读取任务。任务还没有分配、正在 handoff 或本节点不在当前 committee 时保留游标等待；只有本节点属于当前 committee 且任务进入 `Running` 才会读取本节点数据库中的密文/份额并执行 DAG。

计算产生的状态写、已处理记录、游标推进和 result outbox 在一个 PostgreSQL 事务内提交。链上提交失败时 outbox 仍保留，后续提交器可以重试，不会重新执行密态状态变更。当前二进制使用开发明文后端；链上 threshold 签名聚合和 outbox 提交器是下一阶段。
