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
- CI：`.github/workflows/release.yml`

发布流程：

1. 在仓库 Secrets 配置 `NPM_TOKEN`（具发布权限的 npm token）。
2. 打 tag 触发：
   ```bash
   git tag v1.0.0 && git push origin v1.0.0
   ```
3. CI 自动：matrix 交叉编译 5 平台二进制 → `build-npm.mjs` 组装子包/主包并注入版本 →
   先发所有子包、后发主包。

本地预演（不发布）：
```bash
# 先把各 target 的二进制放到 artifacts/<target>/swagger[.exe]
node scripts/build-npm.mjs 1.0.0       # 仅生成 dist-npm/
node scripts/build-npm.mjs 1.0.0 --publish  # 实际发布（需已 npm login）
```

详见 MIGRATION.md §14。
