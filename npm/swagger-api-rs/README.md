# swagger-api-rs

将 Swagger / OpenAPI 接口文档转换为前端代码（**TypeScript / JavaScript / Flutter**）的命令行工具，Rust 实现。

## 安装

```bash
npm i -g swagger-api-rs
```

安装时 npm 会根据当前平台自动拉取对应的预编译二进制（通过 `optionalDependencies`），
无 postinstall、无运行时下载。

## 使用

在项目根目录放置配置文件 `swagger.json` 后执行：

```bash
swagger              # 生成 + 打印本次相对上次的变更
swagger --file=foo   # 读取 swagger.foo.json
swagger --no-diff    # 生成但不打印变更
swagger --diff-only  # 仅预览变更，不写文件
```

配置示例：

```jsonc
{
  "url": "http://your.host",
  "suffix": "/api-admin",
  "output": "src/api",
  "language": "ts",
  "header": ["import { server } from '@/utils/network';"],
  "filter": ["/dev/query"]
}
```

完整说明见仓库 README 与 MIGRATION.md。
