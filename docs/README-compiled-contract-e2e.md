# `.ppsc` 编译 → 链上部署 → 用户调用 → Runtime 回写

## 网页 Wallet 入口

本文默认使用真实 `manifest_crypto_service`（OpenFHE BFV + Shamir），由常驻
`manifest_committee_daemon` 自动处理任务。`/topic-one` 网页演示已接入同一流程，
可自动创建独立 Anvil / PostgreSQL、编译部署合约并启动两个常驻进程。
网页 demo 与 Wallet 一键环境分别使用独立动态端口；本文手工流程使用 8545。

首次准备原生库（C++17 编译器、git、Python/pip、Cargo）：

```bash
python3 scripts/build-real-backend.py
python3 scripts/test-real-crypto.py
```

脚本将固定版本 OpenFHE v1.5.1 与 CMake 放在 `target/`，不安装到系统目录。
密码委员会各方仍在一个进程内运行，H2S / S2H 会解密再分享或重加密；
开发 Gateway 的 opening 公开写入链上，并非端到端私密查询。单节点链上委员会与 accept-all verifier 仅用于本地集成。
原生依赖准备好后，可以直接在 `/wallet` 的概览点击「一键部署本地环境」，替代手工执行第 1–8 步：

```bash
cd front-blockchains-new
npm install
npm run dev -- --hostname 127.0.0.1
```

打开 `http://127.0.0.1:3000/wallet`，在概览启动本地环境。页面会创建隔离 Anvil / PostgreSQL，
编译部署默认 ConfidentialToken、创建 Alice / Bob 账户、启动真实 OpenFHE daemon，并自动连接 Gateway。
合约页支持上传 `.ppsc` → 编译 →「自动部署合约」，每个合约有独立的 ControlPlane 和密码 daemon，
既有部署保持运行。部署记录包含回执、输入上传 URL 和公钥路径；新部署的兼容合约自动接入钱包。
停止环境会终止该钱包创建的全部进程；重新启动使用新链与新密钥。

仍可手工执行本文步骤，通过 8545 连接。使用一键环境时，第 10/14 步应改用部署记录中的
ControlPlane、contractId、公钥路径与上传 URL。也可直接在钱包交易表单输入金额或上传 `.bfv` 文件，
点击「上传并获取 dataId」，完成链上登记后自动填入交易表单。概览的「显示余额 / 刷新余额」
通过 `getBalance` 等待 daemon 返回实际 opening；该开发版 opening 公开写入本地链。
仅连接本机 Anvil / Chain ID 31337；环境会话过期会拒绝请求，不会切到另一条本地链。
详细说明见 [前端 Wallet 使用说明](../front-blockchains-new/README.md#wallet本地链密态账户)。


最终结果：

```text
Alice balance: 0 → 100 → 70
Bob   balance: 0 → 30
```

完整路径：

```text
ConfidentialToken.ppsc
  → manifest.json + ConfidentialTokenGateway.sol
  → Gateway 构造函数在 ControlPlane 发布 manifest 和函数哈希
  → 用户调用 Gateway
  → ControlPlane 产生 ExecutionRequested
  → 常驻 committee daemon 读取任务和 manifest
  → PostgreSQL 中执行密态状态转换
  → daemon 用节点自己的 key 提交结果
  → ControlPlane 更新 balance[user] 的 dataId
  → 用户从 Gateway 读取授权 opening
```



## 0. 终端安排

使用四个终端，且都进入正确项目：

```bash
cd /Users/esion/Downloads/PSCE_hub-main/code/ppsc
```

- 终端 A：Anvil；
- 终端 B：编译、部署、Alice/Bob 链上调用；
- 终端 C：长期运行的 committee daemon；
- 终端 D：用户侧密文上传客户端。

终端 D 使用 Alice/Bob 自己的 `USER_PRIVATE_KEY` 对上传授权签名。HTTP 服务先验签，再把 opaque 密文或秘密分享持久化到 PostgreSQL outbox；常驻 daemon 随后使用自己的 `NODE_TX_KEY` 登记链上 metadata。用户端不再接触 Runtime 节点私钥。

## 1. 准备 PostgreSQL

确认 PostgreSQL 已启动：

```bash
brew services start postgresql@16
pg_isready -h 127.0.0.1 -p 5432
```

预期：

```text
127.0.0.1:5432 - accepting connections
```

首次运行创建节点数据库：

```bash
export DEMO_DB='ppsc_manifest_demo'
createdb "$DEMO_DB"
export DATABASE_URL="postgres://127.0.0.1/$DEMO_DB"
echo "$DATABASE_URL"
```

如果 `createdb` 提示数据库已经存在，说明它可能保留了上一条 Anvil 链的任务游标。请换一个新名字，例如 `ppsc_manifest_demo_2`，并同步修改终端 C、D 的 `DATABASE_URL`。

`PostgresManifestState` 首次连接时会自动执行 `0002_manifest_runtime.sql`，不需要手工建表。

## 2. 启动本地链（终端 A）

```bash
anvil --port 8545 --chain-id 31337
```

本演示固定使用 Anvil 前三个账户：

| 身份 | 地址 | 用途 |
|---|---|---|
| Alice/部署者 | `0xf39F...2266` | 部署、创建账户、入账、转账、查询 |
| Bob | `0x7099...79C8` | 创建账户、查询 |
| Runtime 节点 | `0x3C44...93BC` | committee 成员、提交计算结果 |

## 3. 设置终端 B 环境


```bash
export RPC_URL='http://127.0.0.1:8545'
export ALICE='0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266'
export ALICE_KEY='0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80'
export BOB='0x70997970C51812dc3A010C7d01b50e0d17dc79C8'
export BOB_KEY='0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d'
export NODE_ADDRESS='0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC'
export NODE_TX_KEY='0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a'
export DEADLINE=2000000000
export NO_PROXY='127.0.0.1,localhost'
export no_proxy="$NO_PROXY"
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy
```

这里设置 `NO_PROXY` 并仍在 Foundry 命令中传 `--no-proxy`，用于修复 macOS 系统代理导致的：

```text
Error: HTTP error 502 with empty body
```

## 4. 编译 `.ppsc` 并生成可部署 Gateway（终端 B）

```bash
cargo run -p ppsc-compiler --bin ppsc -- build \
  examples/contracts/ConfidentialToken.ppsc \
  --sol-out contracts/src/generated
```

预期：

```text
compiled examples/contracts/ConfidentialToken.ppsc
artifacts target/ppsc/ConfidentialToken
gateway contracts/src/generated/ConfidentialTokenGateway.sol
```

加载编译器生成的哈希并编译 Solidity：

```bash
source target/ppsc/ConfidentialToken/hashes.env
echo "$MANIFEST_HASH"
echo "$RUNTIME_HASH"
forge build
```

`ConfidentialTokenGateway.sol` 内嵌每个函数的 selector、program hash、operator-sequence hash 和 ABI hash。它的构造函数会把这些编译结果发布到 ControlPlane。

## 5. 部署 verifier 与 ControlPlane（终端 B）

先部署只用于本地 Anvil 的 accept-all verifier：

```bash
forge create \
  contracts/src/examples/LocalAcceptAllSortitionVerifier.sol:LocalAcceptAllSortitionVerifier \
  --rpc-url "$RPC_URL" \
  --private-key "$ALICE_KEY" \
  --broadcast --no-proxy
```

干净 Anvil 的预期地址：

```text
Deployed to: 0x5FbDB2315678afecb367f032d93F642f64180aa3
```

```bash
export VERIFIER='0x5FbDB2315678afecb367f032d93F642f64180aa3'
```

部署 ControlPlane。注意：`--constructor-args` 必须放在最后；否则新版 Forge 会把后续选项也当成构造参数，出现 `expected 3 but got 9/10`。

```bash
forge create contracts/src/PpscControlPlane.sol:PpscControlPlane \
  --rpc-url "$RPC_URL" \
  --private-key "$ALICE_KEY" \
  --broadcast --no-proxy \
  --constructor-args "$ALICE" "$NODE_ADDRESS" "$VERIFIER"
```

干净 Anvil 的预期地址：

```text
Deployed to: 0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512
```

```bash
export CONTROL='0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512'
```

如果地址与预期不同，后续变量必须使用终端实际输出的地址。

## 6. 在链上登记本地 committee（终端 B）

```bash
export ACTIVE_COMMITTEE_ID="$(cast keccak 'PPSC_LOCAL_COMMITTEE_V1')"
export COMMITTEE_SEED="$(cast keccak 'PPSC_LOCAL_SEED_V1')"

cast send "$CONTROL" \
  'finalizeCommittee(bytes32,bytes32,uint64,address[],uint16,bytes)' \
  "$ACTIVE_COMMITTEE_ID" "$COMMITTEE_SEED" 1 \
  "[$NODE_ADDRESS]" 1 0x01 \
  --private-key "$NODE_TX_KEY" \
  --rpc-url "$RPC_URL" --no-proxy
```

这里使用单节点、threshold=1，只验证集成链路。生产环境必须换成多节点 sortition 和阈值签名聚合。

## 7. 部署编译生成的 Gateway（终端 B）

```bash
export DEPLOYMENT_SALT="$(cast keccak 'PPSC_CONFIDENTIAL_TOKEN_DEMO_V1')"
export INITIAL_STATE_ROOT="$(cast keccak 'PPSC_EMPTY_CONFIDENTIAL_STATE_V1')"

forge create \
  contracts/src/generated/ConfidentialTokenGateway.sol:ConfidentialTokenGateway \
  --rpc-url "$RPC_URL" \
  --private-key "$ALICE_KEY" \
  --broadcast --no-proxy \
  --constructor-args \
  "$CONTROL" "$DEPLOYMENT_SALT" "$MANIFEST_HASH" "$RUNTIME_HASH" \
  "$INITIAL_STATE_ROOT" \
  'file://target/ppsc/ConfidentialToken/manifest.json'
```

干净 Anvil 的预期地址：

```text
Deployed to: 0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0
```

```bash
export GATEWAY='0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0'
export CONTRACT_ID="$(cast call "$GATEWAY" \
  'confidentialContractId()(bytes32)' \
  --rpc-url "$RPC_URL" --no-proxy)"
echo "$CONTRACT_ID"
```

再验证链上登记的 manifest hash 确实来自刚才的编译产物：

```bash
cast call "$CONTROL" 'contractManifestHash(bytes32)(bytes32)' \
  "$CONTRACT_ID" --rpc-url "$RPC_URL" --no-proxy
echo "$MANIFEST_HASH"
```

两行必须一致。到这里，编译器输出已经实际部署并登记到链上。

## 8. 启动无人值守 committee daemon（终端 C）

如果你严格按照前面的顺序在干净 Anvil 上部署，地址和 ID 是确定的，直接使用下面这些值：

```bash
cd /Users/esion/Downloads/PSCE_hub-main/code/ppsc

export DATABASE_URL='postgres://127.0.0.1/ppsc_manifest_demo'
export RPC_URL='http://127.0.0.1:8545'
export CONTROL='0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512'
export GATEWAY='0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0'
export CONTRACT_ID='0x5cae2fdffb57ec6d4a47e78cbac6b6c40febd54008acb6289b30919e35576041'
export ACTIVE_COMMITTEE_ID='0x5f3d124d10e955845ad503613444480394b7662a97901e802f17fe945a20e730'
export MANIFEST_PATH='target/ppsc/ConfidentialToken/manifest.json'
export NODE_ADDRESS='0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC'
export NODE_TX_KEY='0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a'
export NODE_ID='local-node-1'
export POLL_INTERVAL_SECONDS=2
export UPLOAD_LISTEN='127.0.0.1:8787'

# 实际 BFV / Shamir 密码运算，密钥在独立长期进程中保持。
export MANIFEST_CRYPTO_BACKEND='process'
export MANIFEST_CRYPTO_COMMAND="$PWD/target/debug/manifest_crypto_service"
export MANIFEST_PUBLIC_KEY_PATH="$PWD/target/manifest-committee.pub"
export NO_PROXY='127.0.0.1,localhost'
export no_proxy="$NO_PROXY"
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy

cargo build -p ppsc-fhe --bin manifest_crypto_service --bin manifest_encrypt_input
cargo build -p ppsc-runtime --bins
cargo run -p ppsc-runtime --bin manifest_committee_daemon
```

预期：

```text
manifest committee daemon started: node=local-node-1 contract=0x... backend=process poll=2s
OpenFHE BFV/Shamir service ready: pid=...
authenticated input upload listening: http://127.0.0.1:8787/v1/inputs
```

确认公钥文件已生成再上传输入。公钥仅包含公开参数；密钥不持久化，daemon / 密码服务退出后需重新创建整套演示环境，旧密文无法由新密钥恢复。保持终端 C 运行，不再手工调用 worker。每次用户发起任务，它会自动输出：

```text
task computed and queued: index=... execution=0x...
result finalized on chain: execution=0x... output=0x...
```

`NODE_TX_KEY` 是 Runtime 节点自己的 key，只用于支付提交交易并表明节点身份；不是 Alice/Bob 的 key。用户通过 Gateway 发起调用时使用自己的 key。

## 9. Alice 和 Bob 调用生成合约创建账户（终端 B）

Alice：

```bash
cast send "$GATEWAY" 'createAccount(uint64,uint64)' 1 "$DEADLINE" \
  --private-key "$ALICE_KEY" --rpc-url "$RPC_URL" --no-proxy
```

Bob：

```bash
cast send "$GATEWAY" 'createAccount(uint64,uint64)' 1 "$DEADLINE" \
  --private-key "$BOB_KEY" --rpc-url "$RPC_URL" --no-proxy
```

观察终端 C；两个任务都应出现 `result finalized on chain`。账户初值来自源码的 `FHE.encrypt(0)`，不是 Alice/Bob 在链上登记的明文余额。

## 10. 上传 Alice 的密态入账值 100（终端 D）

终端 D 需要独立设置环境变量；变量不会自动从终端 C 继承。用户端不需要 `DATABASE_URL`、`RPC_URL`、`NODE_ADDRESS` 或 `NODE_TX_KEY`：

```bash
cd /Users/esion/Downloads/PSCE_hub-main/code/ppsc

export CONTROL='0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512'
export CONTRACT_ID='0x5cae2fdffb57ec6d4a47e78cbac6b6c40febd54008acb6289b30919e35576041'
export UPLOAD_URL='http://127.0.0.1:8787/v1/inputs'
export USER_PRIVATE_KEY='0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80'
```

然后运行：

```bash
target/debug/manifest_encrypt_input target/manifest-committee.pub 100 target/alice-100.bfv
cargo run -p ppsc-runtime --bin manifest_input_client -- \
  fhe-file target/alice-100.bfv 1 2000000000
```

预期：

```text
input accepted: owner=0xf39F... dataId=0x... status=accepted
```

终端 C 随后自动输出：

```text
uploaded input registered on chain: dataId=0x... owner=0xf39f...
```

复制客户端输出的 `dataId`。加密工具只读取委员会公钥，生成二进制 BFV 密文；上传工具使用用户 key 签名。真实后端不接受 `dev-fhe` 的明文标签载荷。金额为整数，单次输入范围为 0–499122176；合约运算也必须保持在 BFV 模数的安全范围内，本例固定使用 100/30。秘密分享 bundle 可用 `ss-file`。

## 11. Alice 调用生成合约入账（终端 B）

```bash
export DEPOSIT_DATA_ID='粘贴上一步dataId'

cast send "$GATEWAY" 'deposit(bytes32,uint64,uint64)' \
  "$DEPOSIT_DATA_ID" 2 "$DEADLINE" \
  --private-key "$ALICE_KEY" --rpc-url "$RPC_URL" --no-proxy
```

这里调用的是编译生成的 `deposit`。Gateway 把密文地址 `dataId` 交给 ControlPlane；终端 C 自动执行 manifest 中的：

```text
state.load balance[msg.sender]
fhe.add(balance, amount)
state.store balance[msg.sender]
```

## 12. 不调用 Runtime，直接查询密态余额引用（终端 B）

这一步只读链上，不触发新计算：

```bash
export ALICE_VARIABLE="$(cast call "$GATEWAY" \
  'balanceVariable(address)(bytes32)' "$ALICE" \
  --rpc-url "$RPC_URL" --no-proxy)"

cast call "$CONTROL" \
  'stateVariables(bytes32)(bytes32,bytes32,uint8,uint32,uint64,bool)' \
  "$ALICE_VARIABLE" --rpc-url "$RPC_URL" --no-proxy
```

输出依次是：`contractId`、当前 `dataId`、representation（`1=FHE`）、output slot、version、exists。这里看不到明文 100，这正是密态余额应有的行为。

## 13. Alice 请求授权 opening，显示转账前余额（终端 B）

先用只读 `eth_call` 取得确定性的 execution ID，再发送真实交易：

```bash
export BEFORE_EXEC="$(cast call "$GATEWAY" \
  'getBalance(uint64,uint64)(bytes32)' 3 "$DEADLINE" \
  --from "$ALICE" --rpc-url "$RPC_URL" --no-proxy)"

cast send "$GATEWAY" 'getBalance(uint64,uint64)' 3 "$DEADLINE" \
  --private-key "$ALICE_KEY" --rpc-url "$RPC_URL" --no-proxy
```

终端 C 出现 `result finalized on chain` 后：

```bash
export RAW_BEFORE="$(cast call "$GATEWAY" \
  'openingResults(bytes32)(bytes)' "$BEFORE_EXEC" \
  --rpc-url "$RPC_URL" --no-proxy)"

cast to-dec "0x${RAW_BEFORE: -32}"
```

预期：

```text
100
```

后 16 字节是开发 backend 的 `u128` 载荷。生产版本不会把 plaintext opening 放在公开 mapping 中，而会返回仅 Alice 可解密的密文。

## 14. 上传密态金额 30，并由 Alice 调用 transfer

终端 D：

```bash
target/debug/manifest_encrypt_input target/manifest-committee.pub 30 target/alice-30.bfv
cargo run -p ppsc-runtime --bin manifest_input_client -- \
  fhe-file target/alice-30.bfv 2 2000000000
```

复制 `dataId` 到终端 B：

```bash
export TRANSFER_DATA_ID='粘贴金额30对应的dataId'

cast send "$GATEWAY" \
  'transfer(address,bytes32,uint64,uint64)' \
  "$BOB" "$TRANSFER_DATA_ID" 4 "$DEADLINE" \
  --private-key "$ALICE_KEY" --rpc-url "$RPC_URL" --no-proxy
```

终端 C 会实际按编译产物执行：

```text
FHE.ge(AliceBalance, amount)
H2S(FheBool)
require(SecretBool)
FHE.sub(AliceBalance, amount)
FHE.add(BobBalance, amount)
state.store Alice
state.store Bob
```

余额不足时 `require` 失败，PostgreSQL 事务不会提交任何部分写入。

## 15. 查询转账后的 Alice=70、Bob=30

Alice 和 Bob 可以先后或同时创建只读 opening；只读任务不会推进合约 state root：

```bash
export ALICE_AFTER_EXEC="$(cast call "$GATEWAY" \
  'getBalance(uint64,uint64)(bytes32)' 5 "$DEADLINE" \
  --from "$ALICE" --rpc-url "$RPC_URL" --no-proxy)"

cast send "$GATEWAY" 'getBalance(uint64,uint64)' 5 "$DEADLINE" \
  --private-key "$ALICE_KEY" --rpc-url "$RPC_URL" --no-proxy

export BOB_AFTER_EXEC="$(cast call "$GATEWAY" \
  'getBalance(uint64,uint64)(bytes32)' 2 "$DEADLINE" \
  --from "$BOB" --rpc-url "$RPC_URL" --no-proxy)"

cast send "$GATEWAY" 'getBalance(uint64,uint64)' 2 "$DEADLINE" \
  --private-key "$BOB_KEY" --rpc-url "$RPC_URL" --no-proxy
```

等终端 C 两次输出 `result finalized on chain`，然后读取：

```bash
export RAW_ALICE="$(cast call "$GATEWAY" \
  'openingResults(bytes32)(bytes)' "$ALICE_AFTER_EXEC" \
  --rpc-url "$RPC_URL" --no-proxy)"
cast to-dec "0x${RAW_ALICE: -32}"

export RAW_BOB="$(cast call "$GATEWAY" \
  'openingResults(bytes32)(bytes)' "$BOB_AFTER_EXEC" \
  --rpc-url "$RPC_URL" --no-proxy)"
cast to-dec "0x${RAW_BOB: -32}"
```

预期：

```text
70
30
```

检查 Bob 的链上执行状态：

```bash
cast call "$CONTROL" 'executionStatus(bytes32)(uint8)' \
  "$BOB_AFTER_EXEC" --rpc-url "$RPC_URL" --no-proxy
```

预期 `6`，表示 `Completed`。

## 16. 检查节点 PostgreSQL

终端 D：

```bash
psql "$DATABASE_URL" -c \
  "SELECT state_name, encode(state_key, 'hex'), value_type, version
   FROM ppsc_manifest_state ORDER BY state_name, state_key;"

psql "$DATABASE_URL" -c \
  "SELECT queue_index, encode(execution_id, 'hex'), outcome
   FROM ppsc_manifest_processed_tasks ORDER BY queue_index;"

psql "$DATABASE_URL" -c \
  "SELECT encode(data_id, 'hex'), encode(owner, 'hex'), registered, attempts
   FROM ppsc_manifest_uploads ORDER BY created_at;"
```

应看到两个 `balance` key，以及每个链上任务对应的 processed record。每个 committee 节点应使用自己的 `DATABASE_URL`；多节点环境不能共用一套私密状态表。

## 17. 常见错误

### `HTTP error 502 with empty body`

Anvil 正常但 Foundry 走了 macOS 系统代理。执行第 3 步的 `NO_PROXY/unset`，并保留每条 RPC 命令的 `--no-proxy`。

### `Constructor argument count mismatch: expected 3 but got 9/10`

把 `--constructor-args` 移到 `forge create` 的最后。它后面只能出现真实构造参数，不能再放 `--rpc-url` 或 `--broadcast`。

### `missing environment variable DATABASE_URL/RPC_URL`

环境变量只在设置它的当前终端有效。终端 C、D 都要各自执行第 8/10 步要求的完整 `export`。

### `NODE_TX_KEY does not match NODE_ADDRESS`

daemon 会主动校验节点 key。这里必须使用 Anvil 账户 2 的地址和私钥；不要填 Alice 的 key。

### `execution reverted` 或 `Failed to estimate gas`

依次检查：

1. `deadline` 是否仍大于链上时间；
2. 同一用户的 nonce 是否已经用过；
3. `dataId` 是否已由上传 outbox 登记、owner 是否与调用者一致；
4. committee 是否 active，成员是否包含 `NODE_ADDRESS`；
5. daemon 的 manifest hash 是否与链上的 `contractManifestHash` 相同；
6. Alice 是否有足够密态余额通过 `FHE.ge → H2S → require`。

## 18. 这条演示证明了什么

- `.ppsc` 编译结果确实被部署和调用，不再停留在 JSON；
- 用户私钥负责 Gateway 调用和上传授权签名，Runtime 使用自己的节点 key 提交；
- 用户传入的是密文 `dataId`，不是在链上直接传余额；
- `balance[address]` 的变量地址由生成 Gateway 和 ControlPlane 规则确定，不由用户随意登记；
- committee 计算完成后负责更新链上变量的新 `dataId/slot/version`；
- 查询密态引用不需要 Runtime，只有授权打开明文才需要 committee；
- PostgreSQL 的状态写、outbox、任务游标和链上结果提交已经串在同一流程中。

## 19. 后端与运行边界

默认已使用 `manifest_crypto_service`。`OPENFHE_DIR` / `OPENFHE_LIB_DIR` 可覆盖原生库路径；
否则优先使用根目录 `openfhe-development/`，再使用构建脚本的 `target/openfhe-development/`。
出现 `openfhe.h file not found` 时先运行 `python3 scripts/build-real-backend.py`，不会自动回退到明文后端。

`manifest_crypto_plaintext_service` 和 `dev-fhe` 仍保留给明确选择的协议测试，不能与真实密文混用。
常驻 daemon 会检测密码子进程是否退出；网页显示 daemon / 密码进程 PID 与公钥指纹，停止环境会关闭整组进程。

现阶段仍非生产系统：密码各方同进程、转换过程会打开值、链上 threshold=1 和 accept-all sortition、
公开 opening、金额范围约束、缺少密钥持久化与分布式公钥认证，都需要在实际部署前完善。
