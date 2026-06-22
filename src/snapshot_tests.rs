//! 端到端快照测试（#12）。
//!
//! 用一份覆盖关键场景的小型 OpenAPI spec，锁定三种语言的完整生成输出，
//! 防止 lower/codegen 回归。另有基于真实 fixture 的不变量测试。

use crate::codegen::for_language;
use crate::config::{Config, Language};
use crate::lower::lower;
use crate::openapi::ApiDoc;

/// 覆盖关键场景的小型 spec：
/// - 对象返回 / 路径参数 / 请求体 / 分页 / 文件上传 / 数组 / Map<Int> / 导出
const MINI_SPEC: &str = r##"{
  "paths": {
    "/user/info": {
      "get": { "summary": "用户信息", "responses": { "200": {
        "content": { "*/*": { "schema": { "$ref": "#/components/schemas/RUserVo" } } } } } }
    },
    "/user/{id}": {
      "get": { "summary": "用户详情",
        "parameters": [{ "name": "id", "schema": { "type": "integer", "format": "int64" } }],
        "responses": { "200": { "content": { "*/*": { "schema": { "$ref": "#/components/schemas/RBoolean" } } } } } }
    },
    "/login": {
      "post": { "summary": "登录",
        "requestBody": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/LoginDto" } } } },
        "responses": { "200": { "content": { "*/*": { "schema": { "$ref": "#/components/schemas/RUserVo" } } } } } }
    },
    "/page": {
      "get": { "summary": "分页",
        "parameters": [{ "name": "keyword", "schema": { "type": "string" } }],
        "responses": { "200": { "content": { "*/*": { "schema": { "$ref": "#/components/schemas/RIPageUserVo" } } } } } }
    },
    "/upload": {
      "post": { "summary": "上传",
        "requestBody": { "content": { "multipart/form-data": { "schema": { "type": "object" } } } },
        "responses": { "200": { "content": { "*/*": { "schema": { "$ref": "#/components/schemas/RVoid" } } } } } }
    },
    "/list": {
      "get": { "summary": "列表", "responses": { "200": {
        "content": { "*/*": { "schema": { "$ref": "#/components/schemas/RListUserVo" } } } } } }
    },
    "/stats": {
      "post": { "summary": "统计", "responses": { "200": {
        "content": { "*/*": { "schema": { "$ref": "#/components/schemas/RMapStringInteger" } } } } } }
    },
    "/report/export": {
      "get": { "summary": "导出报表", "responses": { "200": {
        "content": { "*/*": { "schema": { "$ref": "#/components/schemas/RString" } } } } } }
    }
  },
  "components": { "schemas": {
    "UserVo": { "type": "object", "properties": {
      "id": { "type": "integer", "format": "int64", "description": "ID" },
      "name": { "type": "string", "description": "姓名" },
      "tags": { "type": "array", "items": { "$ref": "#/components/schemas/Tag" } }
    } },
    "Tag": { "type": "object", "properties": { "label": { "type": "string" } } },
    "LoginDto": { "type": "object", "properties": {
      "phone": { "type": "string", "description": "手机号" },
      "code": { "type": "string", "description": "验证码" }
    } }
  } }
}"##;

fn generate(language: Language) -> String {
    let doc: ApiDoc = serde_json::from_str(MINI_SPEC).unwrap();
    let cfg = Config {
        url: "http://h".into(),
        suffix: String::new(),
        output: "out".into(),
        language,
        deprecated: false,
        header: vec![],
        filter: vec![],
    };
    let module = lower(&[doc], &cfg);
    for_language(language).generate(&module)
}

#[test]
fn snapshot_typescript() {
    insta::assert_snapshot!("typescript", generate(Language::Typescript));
}

#[test]
fn snapshot_javascript() {
    insta::assert_snapshot!("javascript", generate(Language::Javascript));
}

#[test]
fn snapshot_flutter() {
    insta::assert_snapshot!("flutter", generate(Language::Flutter));
}

/// 基于真实 fixture 的不变量：无论快照如何，这些性质必须成立。
#[test]
fn fixture_invariants() {
    let doc: ApiDoc = serde_json::from_str(include_str!("../tests/fixtures/zhtc-api-app.json")).unwrap();
    let cfg = Config {
        url: "http://h".into(),
        suffix: String::new(),
        output: "out".into(),
        language: Language::Typescript,
        deprecated: false,
        header: vec![],
        filter: vec![],
    };
    let module = lower(&[doc], &cfg);
    let ts = for_language(Language::Typescript).generate(&module);

    // 1. Java 基础类型名不应泄漏到产物
    for leak in ["[key:string]:Integer", "[key:string]:Boolean", "[key:string]:Long", "[key:string]:Object"] {
        assert!(!ts.contains(leak), "Java 类型泄漏: {leak}");
    }
    // 2. 不应出现「path 参数已插值却又传 {params}」的明显冗余：抽样检查不含未替换的 {id}
    assert!(!ts.contains("/{"), "URL 模板未替换: 仍含 {{");
    // 3. 产物规模合理
    assert!(ts.len() > 10_000);
    assert!(ts.contains("export const "));
}

/// lowering 确定性：同一份 spec 两次 lower 必须产出相同 IR，且自我 diff 为空。
/// 这保证规范化（#21）与重命名启发式（#19）不会引入假变更（diff 抖动）。
#[test]
fn lowering_is_deterministic() {
    let doc: ApiDoc = serde_json::from_str(MINI_SPEC).unwrap();
    let cfg = Config {
        url: "http://h".into(),
        suffix: String::new(),
        output: "out".into(),
        language: Language::Typescript,
        deprecated: false,
        header: vec![],
        filter: vec![],
    };
    let a = lower(&[doc.clone()], &cfg);
    let b = lower(&[doc], &cfg);
    assert_eq!(a, b, "两次 lowering 结果应完全一致");
    assert!(crate::diff::diff(&a, &b).is_empty(), "自我 diff 应为空");
}

/// diff 报告格式快照：覆盖接口/模型的 增/删/改/重命名 各分支。
#[test]
fn snapshot_diff_report() {
    use crate::diff::diff;
    use crate::ir::{IrEndpoint, IrField, IrModel, IrModule, IrParam, IrType, ParamKind};
    use crate::openapi::HttpMethod;
    use crate::report::render;

    let ep = |func: &str, result: IrType, params: Vec<IrParam>| IrEndpoint {
        func_name: func.into(),
        http_method: HttpMethod::Get,
        url: format!("/{func}"),
        summary: Some("s".into()),
        deprecated: false,
        params,
        result,
        is_export: false,
    };
    let model = |name: &str, fields: &[(&str, IrType)]| IrModel {
        name: name.into(),
        fields: fields
            .iter()
            .map(|(n, t)| IrField { name: (*n).into(), ty: t.clone(), description: None })
            .collect(),
    };

    let old = IrModule {
        endpoints: vec![
            ep("get_old_removed", IrType::Bool, vec![]),
            ep("get_modified", IrType::Bool, vec![]),
            ep("get_v1_thing", IrType::Int, vec![]),
        ],
        models: vec![
            model("RemovedVo", &[("a", IrType::Long)]),
            model("OrderVo", &[("amount", IrType::Long), ("old", IrType::Bool)]),
            model("UserVo", &[("id", IrType::Long), ("name", IrType::String)]),
        ],
    };
    let new = IrModule {
        endpoints: vec![
            ep("post_new_added", IrType::Void, vec![]),
            ep("get_modified", IrType::Int, vec![]),
            ep("get_v2_thing", IrType::Int, vec![]),
        ],
        models: vec![
            model("OrderVo", &[("amount", IrType::String), ("remark", IrType::String)]),
            model("UserInfoVo", &[("id", IrType::Long), ("name", IrType::String)]),
        ],
    };

    insta::assert_snapshot!("diff_report", render(&diff(&old, &new), false));
}

/// 离线模拟「两次生成」：cache + diff + report 全链路（替代需联网的端到端运行）。
#[test]
fn two_generation_diff_flow() {
    use crate::cache;
    use crate::diff;
    use crate::ir::{IrField, IrModel, IrModule, IrType};
    use crate::report;

    let tmp = std::env::temp_dir().join(format!("swagger_e2e_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let cfg = Config {
        url: "http://h".into(),
        suffix: String::new(),
        output: tmp.to_string_lossy().into_owned(),
        language: Language::Typescript,
        deprecated: false,
        header: vec![],
        filter: vec![],
    };

    // 第一次：无缓存 → 首次生成
    assert!(cache::load(&cfg).is_none());
    let gen1 = IrModule {
        endpoints: vec![],
        models: vec![IrModel {
            name: "UserVo".into(),
            fields: vec![IrField { name: "id".into(), ty: IrType::Long, description: None }],
        }],
    };
    cache::save(&cfg, &gen1).unwrap();

    // 第二次：模型字段类型变更 + 新增模型
    let gen2 = IrModule {
        endpoints: vec![],
        models: vec![
            IrModel {
                name: "UserVo".into(),
                fields: vec![IrField { name: "id".into(), ty: IrType::Int, description: None }],
            },
            IrModel { name: "OrderVo".into(), fields: vec![] },
        ],
    };
    let old = cache::load(&cfg).unwrap();
    let d = diff::diff(&old, &gen2);
    assert!(!d.is_empty());
    let txt = report::render(&d, false);
    assert!(txt.contains("+ 新增模型 OrderVo"), "{txt}");
    assert!(txt.contains("~ 修改模型 UserVo"), "{txt}");
    assert!(txt.contains("~ 字段 id: long → int"), "{txt}");

    cache::save(&cfg, &gen2).unwrap();
    // 第三次：与上次相同 → 无变更
    let old2 = cache::load(&cfg).unwrap();
    assert!(diff::diff(&old2, &gen2).is_empty());

    let _ = std::fs::remove_dir_all(&tmp);
}
