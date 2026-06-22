//! 配置文件读取与校验（对应原 JS tools.js getConfig）。
//!
//! 配置结构见 MIGRATION.md §3。配置文件位于当前工作目录，
//! 文件名由 CLI 的 `--file` 推导（swagger.json / swagger.{file}.json）。

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// 目标语言。原版只有 `flutter` 与「默认(ts)」两分支；`js` 为本次新增目标。
///
/// 取值映射（§3）：`"flutter"→Flutter`、`"ts"|缺省→Typescript`、`"js"→Javascript`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    Typescript,
    Javascript,
    Flutter,
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.to_ascii_lowercase().as_str() {
            "flutter" => Language::Flutter,
            "js" | "javascript" => Language::Javascript,
            // "ts"/"typescript"/其它未知值都按原版「默认 ts」处理
            _ => Language::Typescript,
        })
    }
}

/// swagger.json 配置。
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// 接口根地址（必填）。
    pub url: String,

    /// 接口后缀，如 `/api-patrol`（可选）。
    #[serde(default)]
    pub suffix: String,

    /// 输出目录，相对当前工作目录（必填）。
    pub output: String,

    /// 目标语言，缺省 Typescript。
    #[serde(default)]
    pub language: Language,

    /// 是否生成已废弃接口，缺省 false。
    #[serde(default)]
    pub deprecated: bool,

    /// 代码头部注入（import 语句等）。
    #[serde(default)]
    pub header: Vec<String>,

    /// 按 path key 精确过滤掉的接口。
    #[serde(default)]
    pub filter: Vec<String>,
}

impl Config {
    /// 从当前工作目录读取并解析配置文件，随后做必填校验（§3）。
    pub fn load(file_name: &str) -> Result<Self> {
        let path = Path::new(file_name);
        let content = fs::read_to_string(path)
            .with_context(|| format!("读取配置文件失败: {file_name}"))?;
        let config: Config = serde_json::from_str(&content)
            .with_context(|| format!("配置文件 JSON 解析失败: {file_name}"))?;
        config.validate()?;
        Ok(config)
    }

    /// 必填项校验：url、output 不能为空。
    fn validate(&self) -> Result<()> {
        if self.url.trim().is_empty() {
            bail!("url 不能为空");
        }
        if self.output.trim().is_empty() {
            bail!("output 不能为空");
        }
        Ok(())
    }

    /// swagger-config 拉取地址：`{url}{suffix}/v3/api-docs/swagger-config`（§2）。
    pub fn swagger_config_url(&self) -> String {
        format!("{}{}/v3/api-docs/swagger-config", self.url, self.suffix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config() {
        let json = r#"{ "url": "http://h", "output": "src/api" }"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.url, "http://h");
        assert_eq!(c.output, "src/api");
        assert_eq!(c.language, Language::Typescript); // 缺省 ts
        assert!(!c.deprecated);
        assert!(c.suffix.is_empty());
        assert!(c.header.is_empty());
        c.validate().unwrap();
    }

    #[test]
    fn language_mapping() {
        let f = |v: &str| -> Language {
            serde_json::from_str::<Config>(&format!(
                r#"{{ "url": "h", "output": "o", "language": "{v}" }}"#
            ))
            .unwrap()
            .language
        };
        assert_eq!(f("flutter"), Language::Flutter);
        assert_eq!(f("js"), Language::Javascript);
        assert_eq!(f("ts"), Language::Typescript);
        assert_eq!(f("unknown"), Language::Typescript); // 未知值回退 ts
    }

    #[test]
    fn validate_rejects_empty_required() {
        let c: Config =
            serde_json::from_str(r#"{ "url": "", "output": "o" }"#).unwrap();
        assert!(c.validate().is_err());
        let c: Config =
            serde_json::from_str(r#"{ "url": "h", "output": "" }"#).unwrap();
        assert!(c.validate().is_err());
    }

    #[test]
    fn builds_swagger_config_url() {
        let c: Config = serde_json::from_str(
            r#"{ "url": "http://h", "suffix": "/api", "output": "o" }"#,
        )
        .unwrap();
        assert_eq!(
            c.swagger_config_url(),
            "http://h/api/v3/api-docs/swagger-config"
        );
    }
}
