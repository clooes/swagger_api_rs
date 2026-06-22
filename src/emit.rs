//! 文件输出（MIGRATION.md §8，对应原 analyze.js saveFile）。
//!
//! 拼接 header + 生成代码，写到 `{output}/index/index.{ext}`。
//!
//! 与原版的差异（有意改进）：原版 `emptyDirSync` 会清空**整个 output 目录**，
//! 这里只清空我们要写入的 `index` 子目录，避免误删用户在 output 下的其它文件。

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::Config;

/// 把生成的代码写入磁盘，返回最终文件路径。
///
/// `code` 已包含内置类型 + 接口 + 模型（由 CodeGenerator::generate 产出）；
/// 这里在最前面拼接配置的 header（import 语句等）。
pub fn write(config: &Config, code: &str, ext: &str) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("获取当前工作目录失败")?;
    let out_dir = cwd.join(&config.output).join("index");

    // 只清空 index 子目录后重建
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)
            .with_context(|| format!("清空输出目录失败: {}", out_dir.display()))?;
    }
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("创建输出目录失败: {}", out_dir.display()))?;

    let file_path = out_dir.join(format!("index.{ext}"));
    let content = assemble(config, code);
    fs::write(&file_path, content)
        .with_context(|| format!("写入文件失败: {}", file_path.display()))?;

    Ok(file_path)
}

/// 拼接最终文件内容：header（按行）+ 空行 + 生成代码。
fn assemble(config: &Config, code: &str) -> String {
    let mut out = String::new();
    if !config.header.is_empty() {
        out.push_str(&config.header.join("\n"));
        out.push_str("\n\n");
    }
    out.push_str(code);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Language;

    fn cfg(header: Vec<&str>) -> Config {
        Config {
            url: "http://h".into(),
            suffix: String::new(),
            output: "out".into(),
            language: Language::Typescript,
            deprecated: false,
            header: header.into_iter().map(String::from).collect(),
            filter: vec![],
        }
    }

    #[test]
    fn assemble_prepends_header() {
        let c = cfg(vec!["import { server } from '@/utils';", "import { T } from '@/types';"]);
        let s = assemble(&c, "export const x = 1;\n");
        assert_eq!(
            s,
            "import { server } from '@/utils';\nimport { T } from '@/types';\n\nexport const x = 1;\n"
        );
    }

    #[test]
    fn assemble_without_header() {
        let c = cfg(vec![]);
        assert_eq!(assemble(&c, "code"), "code");
    }

    #[test]
    fn write_creates_file() {
        // 用临时目录验证写入与清空逻辑
        let tmp = std::env::temp_dir().join(format!("swagger_emit_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let c = cfg(vec!["// header"]);
        let path = write(&c, "// code\n", "ts").unwrap();
        assert!(path.ends_with("out/index/index.ts"));
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "// header\n\n// code\n");

        // 再次写入应清空旧内容（不报错）
        let path2 = write(&c, "// new\n", "ts").unwrap();
        assert_eq!(fs::read_to_string(&path2).unwrap(), "// header\n\n// new\n");

        std::env::set_current_dir(prev).unwrap();
        let _ = fs::remove_dir_all(&tmp);
    }
}
