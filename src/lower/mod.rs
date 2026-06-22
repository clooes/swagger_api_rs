//! Lowering：OpenAPI spec → 语言无关 IR（MIGRATION.md §5、§6、§9）。
//!
//! 所有「Java 类型名 → 语义」的解析都收敛在本模块（及其子模块）一次完成，
//! 各语言 codegen 只消费产出的 IrModule。

mod field_type;
mod normalize;
mod param;
mod result_type;

use crate::config::Config;
use crate::ir::{IrEndpoint, IrField, IrModel, IrModule, IrType, ParamKind};
use crate::openapi::{ApiDoc, HttpMethod, Operation, Schema};

pub use field_type::lower_type;

/// 把若干 api-docs 合并 lower 成一个 IrModule。
///
/// 每个文档先独立 lower，再通过 normalize::merge_into 合并并消解重名冲突（#21）。
pub fn lower(docs: &[ApiDoc], config: &Config) -> IrModule {
    let mut merged = IrModule::new();
    for doc in docs {
        let one = lower_one(doc, config);
        normalize::merge_into(&mut merged, one);
    }
    merged
}

/// 单个文档的 lowering（文档内 schema/接口名天然唯一）。
fn lower_one(doc: &ApiDoc, config: &Config) -> IrModule {
    let mut module = IrModule::new();
    // paths → endpoints
    for (url, item) in &doc.paths {
        // §9：按 path key 过滤
        if config.filter.iter().any(|f| f == url) {
            continue;
        }
        for (method, op) in item.operations() {
            // §9：deprecated 接口在未开启时跳过
            if !config.deprecated && op.deprecated {
                continue;
            }
            module.endpoints.push(lower_endpoint(url, method, op));
        }
    }

    // components.schemas → models
    for (name, schema) in &doc.components.schemas {
        if should_skip_schema(name) {
            continue;
        }
        module.models.push(lower_model(name, schema));
    }
    module
}

fn lower_endpoint(url: &str, method: HttpMethod, op: &Operation) -> IrEndpoint {
    let func_name = make_func_name(method, url);

    let mut params = param::lower_params(op);
    // 回填标量参数的 in_path：名字出现在 url 模板 `{name}` 中即为路径参数
    for p in &mut params {
        if p.kind == ParamKind::Scalar && url.contains(&format!("{{{}}}", p.name)) {
            p.in_path = true;
        }
    }

    // §5.4：导出接口 → 强制 Binary 返回
    let is_export = func_name.to_ascii_lowercase().contains("export")
        || op.summary.as_deref().is_some_and(|s| s.contains("导出"));

    let result = if is_export {
        IrType::Binary
    } else {
        result_type::lower_result_type(op.responses.get("200"))
    };

    IrEndpoint {
        func_name,
        http_method: method,
        url: url.to_string(),
        summary: op.summary.clone(),
        deprecated: op.deprecated,
        params,
        result,
        is_export,
    }
}

/// 函数名生成（§6）：小写方法名 + 规整后的 url。
/// 规整：`/`、`-` → `_`，去除 `{`、`}`。
fn make_func_name(method: HttpMethod, url: &str) -> String {
    let mut s = String::with_capacity(url.len() + 8);
    s.push_str(method.as_lower());
    for c in url.chars() {
        match c {
            '/' | '-' => s.push('_'),
            '{' | '}' => {} // 去除占位大括号
            other => s.push(other),
        }
    }
    s
}

/// 数据模型 lowering（§5、对应 spliceDefinitionsType）。
fn lower_model(name: &str, schema: &Schema) -> IrModel {
    // Dto 模型字段中的枚举视为数字（is_dot=true）
    let is_dto = name.ends_with("Dto");
    let mut fields = Vec::new();
    if let Some(props) = &schema.properties {
        for (key, el) in props {
            fields.push(IrField {
                name: key.clone(),
                ty: lower_type(el, is_dto),
                description: el.description.clone(),
            });
        }
    }
    IrModel {
        name: name.to_string(),
        fields,
    }
}

/// 是否跳过该 schema（§9）。
///
/// 规则（忠实复刻原版，含其 hacky 之处）：
///   - 名称为 `LocalTime` → 跳过（用内置类型）
///   - 第 2 个字符是「严格大写」B..Y（charCode 66..=89）→ 跳过：
///     用于过滤 `R*`（统一响应包装）与 `IPage*` 等泛型实例，它们由 R<T>/IPage 解包，
///     或由 codegen 内置类型提供，不单独生成模型。
///     注意原版用 `>65 && <90`，故第 2 字符恰为 'A'(65) 或 'Z'(90) 时不跳过（保留此 quirk）。
fn should_skip_schema(name: &str) -> bool {
    if name == "LocalTime" {
        return true;
    }
    if let Some(c2) = name.chars().nth(1) {
        let code = c2 as u32;
        if code > 65 && code < 90 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Language};

    fn test_config() -> Config {
        Config {
            url: "http://h".into(),
            suffix: String::new(),
            output: "out".into(),
            language: Language::Typescript,
            deprecated: false,
            header: vec![],
            filter: vec![],
        }
    }

    #[test]
    fn func_name_regularizes_url() {
        assert_eq!(make_func_name(HttpMethod::Get, "/user/info"), "get_user_info");
        assert_eq!(
            make_func_name(HttpMethod::Post, "/user/{id}/detail-info"),
            "post_user_id_detail_info"
        );
    }

    #[test]
    fn skip_schema_rule() {
        assert!(should_skip_schema("LocalTime"));
        assert!(should_skip_schema("RBoolean")); // 'B'=66
        assert!(should_skip_schema("IPageOrderVo")); // 'P'=80
        assert!(!should_skip_schema("AppLoginDto")); // 'p' 小写
        assert!(!should_skip_schema("OrderVo")); // 'r' 小写
    }

    #[test]
    fn lowers_fixture_without_panic() {
        let raw = include_str!("../../tests/fixtures/zhtc-api-app.json");
        let doc: ApiDoc = serde_json::from_str(raw).unwrap();
        let module = lower(&[doc], &test_config());

        // 真实样本：应产出大量 endpoint 与 model
        assert!(module.endpoints.len() > 50, "endpoints={}", module.endpoints.len());
        assert!(module.models.len() > 50, "models={}", module.models.len());

        // 被跳过的包装类型不应出现在 models 中
        assert!(!module.models.iter().any(|m| m.name.starts_with("RIPage")));
        assert!(!module.models.iter().any(|m| m.name == "LocalTime"));

        // 每个 endpoint 函数名非空且以方法名开头
        for ep in &module.endpoints {
            assert!(!ep.func_name.is_empty());
            assert!(
                ["get", "post", "put", "delete"]
                    .iter()
                    .any(|m| ep.func_name.starts_with(m))
            );
        }
    }

    #[test]
    fn deprecated_filtered_unless_enabled() {
        let doc: ApiDoc = serde_json::from_str(
            r##"{"paths":{"/a":{"get":{"deprecated":true,"responses":{}}}},"components":{"schemas":{}}}"##,
        )
        .unwrap();

        let mut cfg = test_config();
        cfg.deprecated = false;
        assert_eq!(lower(std::slice::from_ref(&doc), &cfg).endpoints.len(), 0);

        cfg.deprecated = true;
        assert_eq!(lower(&[doc], &cfg).endpoints.len(), 1);
    }

    #[test]
    fn path_param_marked() {
        let doc: ApiDoc = serde_json::from_str(
            r#"{"paths":{"/user/{id}":{"get":{"parameters":[{"name":"id","schema":{"type":"integer"}}],"responses":{}}}},"components":{"schemas":{}}}"#,
        )
        .unwrap();
        let m = lower(&[doc], &test_config());
        let ep = &m.endpoints[0];
        assert!(ep.params[0].in_path);
    }

    #[test]
    fn export_forces_binary() {
        let doc: ApiDoc = serde_json::from_str(
            r##"{"paths":{"/order/export":{"get":{"responses":{"200":{"content":{"*/*":{"schema":{"$ref":"#/c/RString"}}}}}}}},"components":{"schemas":{}}}"##,
        )
        .unwrap();
        let m = lower(&[doc], &test_config());
        assert!(m.endpoints[0].is_export);
        assert_eq!(m.endpoints[0].result, IrType::Binary);
    }

    #[test]
    fn filter_excludes_path() {
        let doc: ApiDoc = serde_json::from_str(
            r#"{"paths":{"/common/oss/ali":{"get":{"responses":{}}}},"components":{"schemas":{}}}"#,
        )
        .unwrap();
        let mut cfg = test_config();
        cfg.filter = vec!["/common/oss/ali".into()];
        assert_eq!(lower(&[doc], &cfg).endpoints.len(), 0);
    }
}

#[cfg(test)]
mod dump_ts {
    use super::*;
    use crate::codegen::for_language;
    use crate::config::{Config, Language};
    /// 调试用：`cargo test dump -- --ignored` 把真实样本生成的 TS 写到 /tmp 观察。
    #[test]
    #[ignore]
    fn dump() {
        let raw = include_str!("../../tests/fixtures/zhtc-api-app.json");
        let doc: ApiDoc = serde_json::from_str(raw).unwrap();
        let cfg = Config {
            url: "http://h".into(),
            suffix: String::new(),
            output: "out".into(),
            language: Language::Typescript,
            deprecated: false,
            header: vec![],
            filter: vec![],
        };
        let m = lower(&[doc], &cfg);
        let code = for_language(Language::Typescript).generate(&m);
        std::fs::write("/tmp/generated.ts", code).unwrap();
    }
}
