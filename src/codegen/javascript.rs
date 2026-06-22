//! JavaScript 代码生成器（§7.3，新增目标，无 JS 原版）。
//!
//! 基于 TypeScript：去掉所有类型标注（无 interface、无 `:type`、无 `<T>`），
//! 类型信息以 JSDoc 承载（模型用 `@typedef`，接口用 `@param`/`@returns`）。
//! 复用与 TS 相同的 params 决策与两处修正（path 不重复进 query、vo 不进 params）。

use super::CodeGenerator;
use crate::ir::{IrEndpoint, IrField, IrModel, IrType};

/// JavaScript 后端。
pub struct JavascriptGen;

/// params 实参形态（与 TS 同义）。
enum ParamsArg {
    None,
    DotDirect,
    PagingDirect,
    Interface,
}

/// 内置通用类型（以 JSDoc @typedef 表达）。
const BUILTIN: &str = r#"/**
 * @typedef {Object} Paging
 * @property {number} [pageNum]
 * @property {number} [pageSize]
 */

/**
 * @typedef {Object} IPage
 * @property {number} current
 * @property {number} pages
 * @property {Array} records
 * @property {number} size
 * @property {number} total
 */

/**
 * @typedef {Object} MsgType
 * @property {string} node
 * @property {number} type
 * @property {number} [value]
 */
"#;

impl CodeGenerator for JavascriptGen {
    fn map_type(&self, ty: &IrType) -> String {
        match ty {
            IrType::Int => "number".into(),
            IrType::Long => "string".into(),
            IrType::Double => "number".into(),
            IrType::Bool => "boolean".into(),
            IrType::String => "string".into(),
            IrType::Array(inner) => format!("{}[]", self.map_type(inner)),
            IrType::Map(inner) => format!("Object.<string, {}>", self.map_type(inner)),
            IrType::Ref(name) => name.clone(),
            IrType::IPage(inner) => format!("IPage<{}>", self.map_type(inner)),
            IrType::MsgType => "MsgType".into(),
            IrType::File => "File".into(),
            IrType::Binary => "ArrayBuffer".into(),
            IrType::Any => "*".into(),
            IrType::Void => "void".into(),
        }
    }

    fn gen_model(&self, model: &IrModel) -> String {
        let mut out = format!("/**\n * @typedef {{Object}} {}\n", model.name);
        for f in &model.fields {
            let desc = f
                .description
                .as_deref()
                .map(|d| format!(" {d}"))
                .unwrap_or_default();
            out.push_str(&format!(
                " * @property {{{}}} [{}]{}\n",
                self.field_type(f),
                f.name,
                desc
            ));
        }
        out.push_str(" */\n");
        out
    }

    fn gen_endpoint(&self, ep: &IrEndpoint) -> String {
        self.gen_endpoint_impl(ep, &pascal_case(&ep.func_name))
    }

    fn builtin_types(&self) -> &'static str {
        BUILTIN
    }

    fn file_ext(&self) -> &'static str {
        "js"
    }

    /// 覆盖默认组装：参数 @typedef 名若与某模型名冲突则加 `Params` 后缀避让（#21）。
    fn generate(&self, module: &crate::ir::IrModule) -> String {
        let model_names: std::collections::HashSet<&str> =
            module.models.iter().map(|m| m.name.as_str()).collect();
        let mut out = String::new();
        out.push_str(self.builtin_types());
        out.push('\n');
        for ep in &module.endpoints {
            let mut iface = pascal_case(&ep.func_name);
            if model_names.contains(iface.as_str()) {
                iface.push_str("Params");
            }
            out.push_str(&self.gen_endpoint_impl(ep, &iface));
            out.push('\n');
        }
        for model in &module.models {
            out.push_str(&self.gen_model(model));
            out.push('\n');
        }
        out
    }
}

impl JavascriptGen {
    /// 字段类型：描述含「枚举」→ number（§7.1）。
    fn field_type(&self, f: &IrField) -> String {
        if f.is_enum_hint() {
            "number".into()
        } else {
            self.map_type(&f.ty)
        }
    }

    fn params_kind(&self, ep: &IrEndpoint, is_paging: bool) -> ParamsArg {
        let has_scalar = ep.scalar_params().next().is_some();
        let has_file = ep.has_file();
        let has_dot = ep.query_ref_param().is_some();
        if !has_scalar && !has_file && !has_dot && !is_paging {
            return ParamsArg::None;
        }
        if has_dot && !has_scalar && !has_file && !is_paging {
            return ParamsArg::DotDirect;
        }
        if is_paging && !has_scalar && !has_dot && !has_file {
            return ParamsArg::PagingDirect;
        }
        ParamsArg::Interface
    }

    fn gen_endpoint_impl(&self, ep: &IrEndpoint, iface_name: &str) -> String {
        let iface_name = iface_name.to_string();
        let is_paging = ep.is_paging();
        let dot = ep.query_ref_param();
        let body = ep.body_param();
        let has_file = ep.has_file();

        let result_str: Option<String> = if ep.is_export {
            Some("ArrayBuffer".into())
        } else if ep.result.is_void() {
            None
        } else {
            Some(self.map_type(&ep.result))
        };

        let kind = self.params_kind(ep, is_paging);
        // params 的 JSDoc 类型 + 是否需要 @typedef
        let (params_type, typedef): (Option<String>, Option<String>) = match kind {
            ParamsArg::None => (None, None),
            ParamsArg::DotDirect => (Some(self.map_type(&dot.unwrap().ty)), None),
            ParamsArg::PagingDirect => (Some("Paging".into()), None),
            ParamsArg::Interface => (Some(iface_name.clone()), Some(self.gen_typedef(ep, &iface_name, is_paging))),
        };

        let mut out = String::new();

        // 1. 参数对象的 @typedef（Interface 情形）
        if let Some(td) = &typedef {
            out.push_str(td);
        }

        // 2. JSDoc
        out.push_str("/**\n");
        if ep.deprecated {
            out.push_str(" * @deprecated 将于下个版本被弃用\n");
        }
        out.push_str(&format!(" * @description: {}\n", ep.summary.as_deref().unwrap_or("")));
        if let Some(pt) = &params_type {
            out.push_str(&format!(" * @param {{{pt}}} [params]\n"));
        }
        if let Some(b) = body {
            out.push_str(&format!(" * @param {{{}}} [data]\n", self.map_type(&b.ty)));
        }
        out.push_str(" * @return {*}\n */\n");

        // 3. 函数（无类型标注）
        let mut args = Vec::new();
        if params_type.is_some() {
            args.push("params");
        }
        if body.is_some() {
            args.push("data");
        }
        out.push_str(&format!("export const {} = async ({}) => {{\n", ep.func_name, args.join(", ")));
        if has_file {
            out.push_str(FORMDATA_SNIPPET);
        }

        let scalars_have_query = ep.scalar_params().any(|p| !p.in_path);
        let axios = axios_config(has_file, body.is_some(), scalars_have_query, dot.is_some(), is_paging, &result_str);
        out.push_str(&format!(
            "  const res = await server.{} (`{}`{});\n",
            ep.http_method.as_upper(),
            url_template(&ep.url),
            axios,
        ));

        // 4. 返回
        if ep.is_export {
            out.push_str(EXPORT_RETURN);
        } else if let Some(t) = &result_str {
            let suffix = if t.ends_with("[]") { " ?? []" } else { "" };
            out.push_str(&format!("  return res?.result{suffix};\n"));
        } else {
            out.push_str("  return res?.success;\n");
        }
        out.push_str("};\n");
        out
    }

    /// 为 Interface 情形生成参数对象的 @typedef。
    fn gen_typedef(&self, ep: &IrEndpoint, name: &str, is_paging: bool) -> String {
        let mut out = format!("/**\n * @typedef {{Object}} {name}\n");
        if let Some(d) = ep.query_ref_param() {
            // dot：合并引用模型（JSDoc 无继承，用注释提示）
            out.push_str(&format!(" * @property {{{}}} [*] 继承自 {}\n", self.map_type(&d.ty), self.map_type(&d.ty)));
        }
        for p in ep.scalar_params() {
            out.push_str(&format!(" * @property {{{}}} [{}]\n", self.map_type(&p.ty), p.name));
        }
        if is_paging {
            out.push_str(" * @property {number} [pageNum]\n");
            out.push_str(" * @property {number} [pageSize]\n");
        }
        if ep.has_file() {
            out.push_str(" * @property {*} [key]\n");
        }
        out.push_str(" */\n");
        out
    }
}

/// axios 第二参数 config（与 TS 一致：path 参数不进 query；file 用 formdata）。
fn axios_config(
    has_file: bool,
    has_body: bool,
    scalars_have_query: bool,
    has_dot: bool,
    is_paging: bool,
    result_str: &Option<String>,
) -> String {
    let mut items = Vec::new();
    if has_file {
        items.push("data:formdata".to_string());
    }
    if has_body {
        items.push("data".to_string());
    }
    if !has_file && (scalars_have_query || has_dot || is_paging) {
        items.push("params".to_string());
    }
    if result_str.as_deref() == Some("ArrayBuffer") {
        items.push("responseType: 'arraybuffer'".to_string());
    }
    if items.is_empty() {
        String::new()
    } else {
        format!(", {{{}}}", items.join(","))
    }
}

const FORMDATA_SNIPPET: &str = r#"  const formdata = new FormData();
  for (const key in params) {
    if (Object.prototype.hasOwnProperty.call(params, key)) {
      const element = params[key];
      formdata.set(key, element);
    }
  }
"#;

const EXPORT_RETURN: &str = r#"  if (res instanceof ArrayBuffer) {
    return res;
  } else {
    return null;
  }
"#;

/// `/user/{id}` → `/user/${params?.id}`。
fn url_template(url: &str) -> String {
    let mut out = String::new();
    let mut chars = url.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut name = String::new();
            for c2 in chars.by_ref() {
                if c2 == '}' {
                    break;
                }
                name.push(c2);
            }
            out.push_str(&format!("${{params?.{name}}}"));
        } else {
            out.push(c);
        }
    }
    out
}

/// `get_user_info` → `GetUserInfo`。
fn pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut cs = seg.chars();
            match cs.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + &cs.as_str().to_ascii_lowercase(),
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrField, IrModel, IrParam, ParamKind};
    use crate::openapi::HttpMethod;

    fn js() -> JavascriptGen {
        JavascriptGen
    }

    #[test]
    fn model_is_typedef() {
        let g = js();
        let m = IrModel {
            name: "AppLoginDto".into(),
            fields: vec![
                IrField { name: "phone".into(), ty: IrType::String, description: Some("电话".into()) },
                IrField { name: "age".into(), ty: IrType::Int, description: None },
            ],
        };
        let out = g.gen_model(&m);
        assert!(out.contains("@typedef {Object} AppLoginDto"));
        assert!(out.contains("@property {string} [phone] 电话"));
        assert!(out.contains("@property {number} [age]"));
    }

    fn ep(func: &str, method: HttpMethod, url: &str, params: Vec<IrParam>, result: IrType, is_export: bool) -> IrEndpoint {
        IrEndpoint {
            func_name: func.into(),
            http_method: method,
            url: url.into(),
            summary: Some("测试".into()),
            deprecated: false,
            params,
            result,
            is_export,
        }
    }

    #[test]
    fn no_type_annotations_in_code() {
        let g = js();
        let vo = IrParam { name: "vo".into(), ty: IrType::Ref("LoginDto".into()), kind: ParamKind::Body, in_path: false };
        let e = ep("post_login", HttpMethod::Post, "/login", vec![vo], IrType::Ref("UserVo".into()), false);
        let out = g.gen_endpoint(&e);
        // 代码里无类型标注、无泛型
        assert!(out.contains("export const post_login = async (data) =>"), "{out}");
        assert!(out.contains("server.POST (`/login`"));
        assert!(!out.contains("<UserVo>"));
        // 类型在 JSDoc
        assert!(out.contains("@param {LoginDto} [data]"));
        assert!(out.contains(", {data}"));
    }

    #[test]
    fn path_param_not_in_query() {
        let g = js();
        let id = IrParam { name: "id".into(), ty: IrType::Long, kind: ParamKind::Scalar, in_path: true };
        let e = ep("get_user_id", HttpMethod::Get, "/user/{id}", vec![id], IrType::Bool, false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("`/user/${params?.id}`"), "{out}");
        assert!(!out.contains("{params}"), "path 参数不应进 query: {out}");
        assert!(out.contains("export const get_user_id = async (params) =>"));
    }

    #[test]
    fn array_result_default() {
        let g = js();
        let e = ep("get_list", HttpMethod::Get, "/list", vec![], IrType::Array(Box::new(IrType::Ref("Vo".into()))), false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("return res?.result ?? [];"));
        assert!(!out.contains("<Vo[]>"));
    }

    #[test]
    fn export_arraybuffer_no_cast() {
        let g = js();
        let e = ep("get_export", HttpMethod::Get, "/export", vec![], IrType::Binary, true);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("responseType: 'arraybuffer'"));
        assert!(out.contains("return res;"));
        assert!(!out.contains("as ArrayBuffer"));
    }

    #[test]
    fn void_returns_success() {
        let g = js();
        let e = ep("post_do", HttpMethod::Post, "/do", vec![], IrType::Void, false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("return res?.success;"));
    }
}
