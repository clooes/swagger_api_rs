//! IR 缓存读写（#14）。
//!
//! 在输出目录维护 `{output}/.swagger-ir.json`，保存上次生成的（规范化后）IrModule，
//! 供下次生成时做 diff。读取采取「尽力而为」策略：文件缺失 / JSON 损坏 / 版本不符
//! 一律降级为 None（视为首次生成），不报错中断流程。

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::ir::{CACHE_VERSION, IrCache, IrModule};

/// 缓存文件名（位于 output 目录下）。
const CACHE_FILE: &str = ".swagger-ir.json";

/// 缓存文件路径：`{cwd}/{output}/.swagger-ir.json`。
fn cache_path(config: &Config) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("获取当前工作目录失败")?;
    Ok(cwd.join(&config.output).join(CACHE_FILE))
}

/// 读取上次缓存的 IR。任何异常（缺失/损坏/版本不符）→ None（首次生成）。
pub fn load(config: &Config) -> Option<IrModule> {
    let path = cache_path(config).ok()?;
    let content = fs::read_to_string(&path).ok()?;
    let cache: IrCache = serde_json::from_str(&content).ok()?;
    if cache.version != CACHE_VERSION {
        // 版本不兼容 → 当作没有缓存
        return None;
    }
    Some(cache.module)
}

/// 写入本次 IR 缓存（带版本号）。
pub fn save(config: &Config, module: &IrModule) -> Result<()> {
    let path = cache_path(config)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)
            .with_context(|| format!("创建缓存目录失败: {}", dir.display()))?;
    }
    let cache = IrCache::new(module.clone());
    let json = serde_json::to_string_pretty(&cache).context("序列化 IR 缓存失败")?;
    fs::write(&path, json).with_context(|| format!("写入缓存失败: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Language;
    use crate::ir::{IrModel, IrType};

    fn cfg_in(dir: &std::path::Path) -> Config {
        // output 用绝对路径下的子目录，避免依赖 cwd
        Config {
            url: "http://h".into(),
            suffix: String::new(),
            output: dir.to_string_lossy().into_owned(),
            language: Language::Typescript,
            deprecated: false,
            header: vec![],
            filter: vec![],
        }
    }

    fn sample_module() -> IrModule {
        IrModule {
            endpoints: vec![],
            models: vec![IrModel { name: "UserVo".into(), fields: vec![] }],
        }
    }

    #[test]
    fn save_then_load_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("swagger_cache_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let config = cfg_in(&tmp);

        // 首次：无缓存 → None
        assert!(load(&config).is_none());

        let m = sample_module();
        save(&config, &m).unwrap();
        let loaded = load(&config).unwrap();
        assert_eq!(loaded, m);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn corrupt_cache_degrades_to_none() {
        let tmp = std::env::temp_dir().join(format!("swagger_cache_corrupt_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let config = cfg_in(&tmp);

        fs::write(tmp.join(CACHE_FILE), "{ not valid json ").unwrap();
        assert!(load(&config).is_none(), "损坏的缓存应降级为 None");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn version_mismatch_degrades_to_none() {
        let tmp = std::env::temp_dir().join(format!("swagger_cache_ver_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let config = cfg_in(&tmp);

        // 构造一个 version 不同的缓存
        let bad = format!(
            r#"{{ "version": {}, "module": {{ "endpoints": [], "models": [] }} }}"#,
            CACHE_VERSION + 99
        );
        fs::write(tmp.join(CACHE_FILE), bad).unwrap();
        assert!(load(&config).is_none(), "版本不符应降级为 None");

        // 健全性：确保 IrType 引用未被优化掉（占位使用）
        let _ = IrType::Void;

        let _ = fs::remove_dir_all(&tmp);
    }
}
