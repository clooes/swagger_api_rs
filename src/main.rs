//! swagger_api —— Swagger/OpenAPI → 多语言前端代码生成器（Rust 重构版）。
//!
//! Pipeline（详见 MIGRATION.md §0、§11）：
//!   CLI 解析 → 读配置 → HTTP 拉取 spec → lowering(spec→IR) → codegen(IR→代码) → 写文件
//!
//! 本文件为 #1 骨架：打通 CLI 与主流程框架，后续模块逐步填充。

mod cache;
mod cli;
mod codegen;
mod config;
mod diff;
mod emit;
mod fetcher;
// IR 的部分辅助方法供测试/未来使用，暂允许未使用以保持构建输出干净。
#[allow(dead_code)]
mod ir;
mod lower;
mod openapi;
mod report;
#[cfg(test)]
mod snapshot_tests;

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;
use crate::config::Config;

fn main() -> Result<()> {
    let cli = Cli::parse();
    run(cli)
}

fn run(cli: Cli) -> Result<()> {
    let config_file = cli.config_file_name();
    println!("读取配置文件: {config_file}");

    let config = Config::load(&config_file)?;

    let docs = fetcher::fetch_all(&config)?;
    println!(
        "已拉取 {} 个 api-docs，共 {} 个 path",
        docs.len(),
        docs.iter().map(|d| d.paths.len()).sum::<usize>()
    );

    let module = lower::lower(&docs, &config);
    println!(
        "lowering 完成：{} 个接口，{} 个模型",
        module.endpoints.len(),
        module.models.len()
    );

    // 读取上次缓存，用于生成后做 diff
    let previous = cache::load(&config);

    // --diff-only：仅预览变更，不写代码文件、不更新缓存
    if cli.diff_only {
        match &previous {
            Some(old) => report::print(&diff::diff(old, &module)),
            None => println!("首次生成（无历史缓存，无可对比的变更）"),
        }
        return Ok(());
    }

    let generator = codegen::for_language(config.language);
    let code = generator.generate(&module);
    let path = emit::write(&config, &code, generator.file_ext())?;
    println!("已生成文件: {}", path.display());

    // 变更报告（--no-diff 时跳过），首次无缓存则提示
    if !cli.no_diff {
        match &previous {
            Some(old) => report::print(&diff::diff(old, &module)),
            None => println!("首次生成（无历史缓存，跳过 diff）"),
        }
    }

    // emit 成功后写回缓存（避免失败时留下脏缓存）
    cache::save(&config, &module)?;
    Ok(())
}
