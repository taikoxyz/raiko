# Taiko-Reth 迁移计划

## 概述
将 raiko 项目从分散的 taiko-reth 组件迁移到新的单体 taiko-reth 库。

## 当前状态
- 使用分散的 reth 组件（reth-primitives, reth-evm, reth-revm 等）
- 版本：v1.0.0-rc.2-taiko
- alloy 版本：v0.7.2
- revm 版本：v36-taiko

## 目标状态
- 使用新的单体 taiko-reth 库
- 版本：最新（基于 Reth v1.6.0）
- alloy 版本：v1.0.23
- revm 版本：v7.0.1

## 迁移步骤

### 1. 更新根 Cargo.toml

#### 移除旧的 reth 依赖
```toml
# 移除这些依赖
reth-primitives = { git = "https://github.com/taikoxyz/taiko-reth.git", branch = "v1.0.0-rc.2-taiko" }
reth-evm-ethereum = { git = "https://github.com/taikoxyz/taiko-reth.git", branch = "v1.0.0-rc.2-taiko" }
reth-evm = { git = "https://github.com/taikoxyz/taiko-reth.git", branch = "v1.0.0-rc.2-taiko" }
reth-rpc-types = { git = "https://github.com/taikoxyz/taiko-reth.git", branch = "v1.0.0-rc.2-taiko" }
reth-revm = { git = "https://github.com/taikoxyz/taiko-reth.git", branch = "v1.0.0-rc.2-taiko" }
reth-chainspec = { git = "https://github.com/taikoxyz/taiko-reth.git", branch = "v1.0.0-rc.2-taiko" }
reth-provider = { git = "https://github.com/taikoxyz/taiko-reth.git", branch = "v1.0.0-rc.2-taiko" }
```

#### 添加新的 taiko-reth 依赖
```toml
# 添加新的单体依赖
taiko-reth = { git = "https://github.com/TatsujinLabs/taiko-reth.git" }
```

#### 更新 alloy 依赖
```toml
# 更新到新版本
alloy-rlp = { version = "0.3.10", default-features = false, features = ["core-net"] }
alloy-rlp-derive = { version = "0.3.10", default-features = false }
alloy-core = { version = "1.0.23", default-features = false }
alloy-dyn-abi = { version = "1.0.23", default-features = false }
alloy-json-abi = { version = "1.0.23", default-features = false }
alloy-primitives = { version = "1.0.23", default-features = false }
alloy-sol-types = { version = "1.0.23", default-features = false }
alloy-rpc-types = { version = "1.0.23", default-features = false }
alloy-rpc-client = { version = "1.0.23", default-features = false }
alloy-consensus = { version = "1.0.23", default-features = false, features = ["serde"] }
alloy-network = { version = "1.0.23", default-features = false, features = ["k256"] }
alloy-contract = { version = "1.0.23", default-features = false }
alloy-eips = { version = "1.0.23", default-features = false, features = ["serde"] }
alloy-provider = { version = "1.0.23", default-features = false, features = ["reqwest"] }
alloy-transport-http = { version = "1.0.23", default-features = false, features = ["reqwest"] }
alloy-signer = { version = "1.0.23", default-features = false }
alloy-signer-local = { version = "1.0.23", default-features = false }
```

#### 更新 patch 部分
```toml
[patch.crates-io]
# 移除旧的 revm patch
# revm = { git = "https://github.com/taikoxyz/revm.git", branch = "v36-taiko" }
# revm-primitives = { git = "https://github.com/taikoxyz/revm.git", branch = "v36-taiko" }
# revm-precompile = { git = "https://github.com/taikoxyz/revm.git", branch = "v36-taiko" }

# 更新 alloy patch 到新版本
alloy-serde = { git = "https://github.com/taikoxyz/alloy.git", branch = "v1.0.23" }
alloy-rpc-types-eth = { git = "https://github.com/taikoxyz/alloy.git", branch = "v1.0.23" }
alloy-network = { git = "https://github.com/taikoxyz/alloy.git", branch = "v1.0.23" }
alloy-signer-local = { git = "https://github.com/taikoxyz/alloy.git", branch = "v1.0.23" }
alloy-signer = { git = "https://github.com/taikoxyz/alloy.git", branch = "v1.0.23" }
alloy-rpc-types-beacon = { git = "https://github.com/taikoxyz/alloy.git", branch = "v1.0.23" }
alloy-eips = { git = "https://github.com/taikoxyz/alloy.git", branch = "v1.0.23" }
```

### 2. 更新子模块 Cargo.toml

#### lib/Cargo.toml
```toml
[dependencies]
# 替换 reth 依赖
taiko-reth = { workspace = true }

# 更新 alloy 依赖
alloy-rlp = { workspace = true }
alloy-eips = { workspace = true }
alloy-rlp-derive = { workspace = true }
alloy-sol-types = { workspace = true }
alloy-primitives = { workspace = true }
alloy-rpc-types = { workspace = true }
alloy-consensus = { workspace = true }
```

#### core/Cargo.toml
```toml
[dependencies]
# 替换 reth 依赖
taiko-reth = { workspace = true }

# 更新 alloy 依赖
alloy-rlp = { workspace = true }
alloy-rlp-derive = { workspace = true }
alloy-sol-types = { workspace = true }
alloy-primitives = { workspace = true }
alloy-rpc-types = { workspace = true }
alloy-provider = { workspace = true }
alloy-transport-http = { workspace = true }
alloy-consensus = { workspace = true }
alloy-network = { workspace = true }
alloy-rpc-client = { workspace = true }
```

### 3. 代码迁移

#### 导入语句更新
```rust
// 旧版本
use reth_primitives::{Address, Block, Header, TransactionSigned, U256, B256};
use reth_evm::ConfigureEvm;
use reth_revm::Database;

// 新版本
use taiko_reth::{
    primitives::{Address, Block, Header, TransactionSigned, U256, B256},
    evm::ConfigureEvm,
    revm::Database,
};
```

#### API 变更处理
- 检查所有 reth 相关的 API 调用
- 更新可能变更的接口
- 处理版本兼容性问题

### 4. 测试和验证

#### 编译测试
```bash
cargo check
cargo build
```

#### 功能测试
- 运行现有的测试套件
- 验证核心功能正常工作
- 检查性能回归

#### 集成测试
- 测试与外部系统的集成
- 验证 RPC 接口
- 检查状态同步

## 风险点

### 1. API 变更
- Reth v1.6.0 可能有重大 API 变更
- 需要仔细检查所有 reth 相关的代码

### 2. 版本兼容性
- alloy v1.0.23 与 v0.7.2 有重大差异
- 需要更新所有 alloy 相关的代码

### 3. 性能影响
- 新版本可能有性能变化
- 需要基准测试验证

### 4. 依赖冲突
- 可能存在依赖版本冲突
- 需要解决 Cargo.lock 冲突

## 回滚计划

如果迁移失败：
1. 保留当前分支作为备份
2. 创建新的迁移分支
3. 逐步迁移，每次提交可回滚
4. 保持主分支稳定

## 时间估计

- 依赖更新：1-2 天
- 代码迁移：3-5 天
- 测试和调试：2-3 天
- 总计：6-10 天 