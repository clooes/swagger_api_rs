//! 命令行参数解析（对应原 JS tools.js 的 getNodeParams）。
//!
//! 原版用法：`swagger --file=flutter` 读取 `swagger.flutter.json`。
//! 这里用 clap 提供等价能力，并兼容原 `--file=xxx` 写法。

use clap::Parser;

/// Swagger/OpenAPI → 多语言前端代码生成器。
#[derive(Parser, Debug)]
#[command(name = "swagger", version, about = "Swagger API 代码生成器 (Rust)")]
pub struct Cli {
    /// 配置文件后缀：传入 `xxx` 时读取 `swagger.xxx.json`，缺省读取 `swagger.json`。
    #[arg(long)]
    pub file: Option<String>,

    /// 跳过变更对比：仍全量重写文件并更新缓存，但不计算/打印 diff。
    #[arg(long, conflicts_with = "diff_only")]
    pub no_diff: bool,

    /// 仅预览变更：只计算并打印 diff，不写代码文件、不更新缓存。
    #[arg(long)]
    pub diff_only: bool,
}

impl Cli {
    /// 根据 `--file` 推导配置文件名，对应原版 `swagger{.file}.json`。
    pub fn config_file_name(&self) -> String {
        match &self.file {
            Some(f) if !f.is_empty() => format!("swagger.{f}.json"),
            _ => "swagger.json".to_string(),
        }
    }
}
