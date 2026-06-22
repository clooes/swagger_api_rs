# swagger_api

将 Swagger / OpenAPI 接口文档转换为前端代码（**TypeScript / JavaScript / Flutter(Dart)**）的命令行工具，Rust 实现。

是 npm 包 `swagger-api-ts` 的 Rust 重构版：采用「编译器式」三段架构，先把 Swagger 解析成**语言无关的中间表示（IR）**，再由各语言后端生成代码。新增一门语言只需实现一个 trait，不改动解析逻辑。

## 架构

```
Swagger JSON ──解析──> OpenAPI 模型 ──lowering──> IR(语言无关) ──codegen──> TS / JS / Dart
```

| 模块 | 职责 |
|------|------|
| `config` | 读取/校验配置文件 |
| `fetcher` | HTTP 拉取 swagger-config 与各 api-docs |
| `openapi` | Swagger/OpenAPI spec 反序列化 |
| `ir` | 语言无关中间表示（IrType/IrModel/IrEndpoint） |
| `lower` | spec → IR，**所有类型映射规则收敛于此** |
| `codegen` | IR → 各语言代码（实现 `CodeGenerator` trait） |
| `emit` | 拼接 header 并写文件 |

详细设计与类型映射规则见 [MIGRATION.md](./MIGRATION.md)。

## 命令速查

```bash
# ── 构建 ──
cargo build                      # debug 构建 → target/debug/swagger
cargo build --release            # release 构建（strip+lto）→ target/release/swagger
cargo install --path .           # 安装到本机（二进制名 swagger）

# ── 测试 ──
cargo test                       # 全部测试
cargo test <用例名片段>          # 跑匹配的用例（如 cargo test diff）
cargo test -- --nocapture        # 显示测试内 println
INSTA_UPDATE=always cargo test   # 更新快照（改了生成逻辑后，review 后提交）

# ── 使用（在含 swagger.json 的项目目录）──
swagger                          # 生成 + 打印本次变更
swagger --file=foo               # 读取 swagger.foo.json
swagger --no-diff                # 生成但不打印变更
swagger --diff-only              # 仅预览变更，不写文件/缓存
swagger --help / --version

# 代理后无法访问目标域名时（reqwest 尊重 NO_PROXY）
NO_PROXY=your.host swagger

# ── npm 本地预演（不发布）──
# 先把各 target 二进制放到 artifacts/<target>/swagger[.exe]
node scripts/build-npm.mjs 1.0.0           # 仅生成 dist-npm/
node scripts/build-npm.mjs 1.0.0 --publish # 实际发布（需 npm login）

# ── 发布新版本（CI 自动）──
# 1) 对齐 Cargo.toml 的 version 与 tag
# 2) 打 tag 触发 GitHub Actions：
git tag v1.0.0 && git push origin v1.0.0
# 重新触发同一 tag（必须先删远端 tag 再推，force-push 不触发）：
git push origin :refs/tags/v1.0.0 && git push origin v1.0.0
```

## 安装

```bash
cargo install --path .
# 或构建后使用 ./target/release/swagger
cargo build --release
```

二进制名为 `swagger`。

## 使用

在项目根目录放置配置文件 `swagger.json`，执行：

```bash
swagger
```

使用其它配置文件（如 `swagger.flutter.json`）：

```bash
swagger --file=flutter
```

### 变更对比（增量 diff）

每次生成会与上次结果对比，在控制台打印语义变更（新增/删除/修改/重命名的接口与模型），
并把本次 IR 快照写入 `{output}/.swagger-ir.json` 供下次对比。

```bash
swagger              # 生成 + 打印变更
swagger --no-diff    # 生成但不打印变更（仍更新缓存）
swagger --diff-only  # 仅预览变更，不写代码文件、不更新缓存
```

`--no-diff` 与 `--diff-only` 互斥。

### 配置文件

```jsonc
{
  "url": "http://your.host",        // 必填，接口根地址
  "suffix": "/api-admin",            // 可选，接口后缀
  "output": "src/api",               // 必填，输出目录（相对当前目录）
  "language": "ts",                  // 可选：ts(默认) | js | flutter
  "deprecated": false,               // 可选，是否生成已废弃接口
  "header": [                        // 代码头部注入（import 等）
    "import { server } from '@/utils/network';"
  ],
  "filter": ["/dev/query"]           // 可选，按 path 过滤掉的接口
}
```

工具会拉取 `{url}{suffix}/v3/api-docs/swagger-config`，再拉取其中各分组文档，
生成到 `{output}/index/index.{ts|js|dart}`。

> **代理提示**：若处于会拦截目标域名的本地代理后，可设 `NO_PROXY=your.host swagger`
> （reqwest 默认尊重 `NO_PROXY`）。

## 开发

```bash
cargo test            # 运行全部单元/集成/快照测试
cargo test snapshot_  # 仅快照测试
```

快照测试用 [insta](https://insta.rs)：修改生成逻辑后，用 `INSTA_UPDATE=always cargo test` 更新
`src/snapshots/`，并 review 差异后提交。

## 发布到 npm

采用预编译二进制 + optionalDependencies 方案（同 Biome/esbuild），由 GitHub Actions 自动发布。

- 主包：`swagger-api-rs`（`npm/swagger-api-rs/`，含 JS 启动器 `bin/swagger.js`）
- 平台子包：`swagger-api-rs-<os>-<cpu>`（由 `scripts/build-npm.mjs` 从编译产物生成）
  - 5 个：`darwin-arm64` / `darwin-x64` / `linux-x64` / `linux-arm64` / **`windows-x64`**
  - ⚠️ Windows 包名段用 `windows` 而非 `win32`（见下文踩坑），启动器内部把
    `process.platform === "win32"` 映射到 `windows`
- CI：`.github/workflows/release.yml`

### 一次性准备

1. 在 npm 生成 **Automation** 类型的 token（npm → Access Tokens → Generate New Token →
   Classic Token → **Automation**）。必须是 Automation 类型，否则 CI 发布会因 2FA 报 `EOTP`。
2. GitHub 仓库 → Settings → Secrets and variables → Actions → 新增 secret **`NPM_TOKEN`**，
   值为上面的 token。

### 发布一个版本

1. 对齐版本号（保持 `Cargo.toml` 的 `version` 与要发布的 tag 一致，否则 `swagger --version` 不符）。
2. 打 tag 并推送触发：
   ```bash
   git tag v1.0.0 && git push origin v1.0.0
   ```
3. CI 自动：matrix 编译 5 平台二进制（含原生 arm64 runner）→ `build-npm.mjs` 组装子包/主包
   并注入版本 → **先发所有子包、后发主包**（保证用户装主包时 optionalDeps 已存在）。
4. 发布脚本是**幂等**的：已存在的 `包@版本` 会跳过，失败后可安全重跑。

> 重新触发同一 tag：tag 没移动时 force-push 不会触发，需先删再推：
> ```bash
> git push origin :refs/tags/v1.0.0 && git push origin v1.0.0
> ```

### 本地预演（不发布）

```bash
# 先把各 target 的二进制放到 artifacts/<target>/swagger[.exe]
node scripts/build-npm.mjs 1.0.0            # 仅生成 dist-npm/
node scripts/build-npm.mjs 1.0.0 --publish  # 实际发布（需已 npm login）
```

### 踩坑记录（已在当前配置中解决）

| 现象 | 原因 | 解决 |
|------|------|------|
| `EOTP` 要求一次性密码 | NPM_TOKEN 不是 Automation 类型 / 账号开了写操作 2FA | 用 **Automation** token |
| `win32` 子包 `E403 spam detection` | npm 反垃圾拦截包名中的 `win32` | 包名段改用 `windows` |
| `cross` 编译 linux-arm64 失败 | cross 容器内 Rust 太旧，不支持 edition 2024 | 改用 GitHub 原生 `ubuntu-24.04-arm` runner |
| 重跑在已发布包上中断 | npm 不允许覆盖同版本 | 脚本幂等，跳过已存在版本 |

详见 MIGRATION.md §14。
