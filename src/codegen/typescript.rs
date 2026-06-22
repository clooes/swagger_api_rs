//! TypeScript 代码生成器（对应原 splice.js + const.js TsxOtherType）。
//!
//! 仅做「IR → TS 语法」翻译；所有类型语义已在 lower 阶段固化。

use super::CodeGenerator;
use crate::ir::{IrEndpoint, IrField, IrModel, IrParam, IrType};

/// TypeScript 后端。
pub struct TypescriptGen;

/// params 实参形态。
enum ParamsArg {
    /// 不需要 params 实参。
    None,
    /// 直接用单个 dot（query 引用模型）类型。
    DotDirect,
    /// 直接用 Paging 类型。
    PagingDirect,
    /// 需要生成具名 interface。
    Interface,
}

/// 内置通用类型（const.js TsxOtherType）。
const BUILTIN: &str = r#"export interface Paging {
  pageNum?: number;
  pageSize?: number;
  [key: string]: any;
}

export interface IPage<T> {
  current: number;
  pages: number;
  records: T[];
  size: number;
  total: number;
}

export interface MsgType {
  node: string;
  type: number;
  value?: number;
}
"#;

impl CodeGenerator for TypescriptGen {
    fn map_type(&self, ty: &IrType) -> String {
        match ty {
            IrType::Int => "number".into(),
            IrType::Long => "string".into(),
            IrType::Double => "number".into(),
            IrType::Bool => "boolean".into(),
            IrType::String => "string".into(),
            IrType::Array(inner) => format!("{}[]", self.map_type(inner)),
            IrType::Map(inner) => format!("{{[key:string]:{}}}", self.map_type(inner)),
            IrType::Ref(name) => name.clone(),
            IrType::IPage(inner) => format!("IPage<{}>", self.map_type(inner)),
            IrType::MsgType => "MsgType".into(),
            IrType::File => "File".into(),
            IrType::Binary => "ArrayBuffer".into(),
            IrType::Any => "any".into(),
            IrType::Void => "void".into(),
        }
    }

    fn gen_model(&self, model: &IrModel) -> String {
        let mut out = format!("export interface {} {{\n", model.name);
        for f in &model.fields {
            if let Some(desc) = &f.description {
                out.push_str(&format!("  /** {desc} */\n"));
            }
            out.push_str(&format!("  {}?: {};\n", f.name, self.field_type(f)));
        }
        out.push_str("}\n");
        out
    }

    fn gen_endpoint(&self, ep: &IrEndpoint) -> String {
        self.gen_endpoint_impl(ep, &pascal_case(&ep.func_name))
    }

    fn builtin_types(&self) -> &'static str {
        BUILTIN
    }

    fn file_ext(&self) -> &'static str {
        "ts"
    }

    /// 覆盖默认组装：参数 interface 名若与某模型名冲突则加 `Params` 后缀避让（#21）。
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

impl TypescriptGen {
    /// 字段类型：描述含「枚举」时强制为 number（§7.1）。
    fn field_type(&self, f: &IrField) -> String {
        if f.is_enum_hint() {
            "number".into()
        } else {
            self.map_type(&f.ty)
        }
    }

    fn gen_endpoint_impl(&self, ep: &IrEndpoint, iface_name: &str) -> String {
        // 各类参数
        let scalars: Vec<&IrParam> = ep.scalar_params().collect();
        let body = ep.body_param();
        let dot = ep.query_ref_param();
        let has_file = ep.has_file();
        let is_paging = ep.is_paging();

        // 返回类型字符串（Void → None；导出 → ArrayBuffer）
        let result_str: Option<String> = if ep.is_export {
            Some("ArrayBuffer".into())
        } else if ep.result.is_void() {
            None
        } else {
            Some(self.map_type(&ep.result))
        };

        let mut out = String::new();

        // 1. 参数 interface（按需）
        if let Some(iface) = self.params_interface(ep, iface_name, is_paging) {
            out.push_str(&iface);
            out.push('\n');
        }

        // 2. JSDoc
        let sig = self.params_decl(ep, iface_name, is_paging);
        out.push_str("/**\n");
        if ep.deprecated {
            out.push_str(" * @deprecated 将于下个版本被弃用\n");
        }
        out.push_str(&format!(
            " * @description: {}\n",
            ep.summary.as_deref().unwrap_or("")
        ));
        for part in sig.split(',').filter(|s| !s.is_empty()) {
            if let Some((name, ty)) = part.split_once(':') {
                out.push_str(&format!(" * @param {{{}}} {}\n", ty.trim(), name.trim()));
            }
        }
        out.push_str(" * @return {*}\n */\n");

        // 3. 函数体
        out.push_str(&format!("export const {} = async ({}) => {{\n", ep.func_name, sig));
        if has_file {
            out.push_str(FORMDATA_SNIPPET);
        }
        let generic = result_str
            .as_ref()
            .map(|t| format!("<{t}>"))
            .unwrap_or_default();
        let axios = self.axios_config(has_file, body.is_some(), &scalars, dot.is_some(), is_paging, &result_str);
        out.push_str(&format!(
            "  const res = await server.{}{} (`{}`{});\n",
            ep.http_method.as_upper(),
            generic,
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

    /// 生成参数 interface（§7.1 paramsInterface）。返回 None 表示不需要。
    ///
    /// 关键：`vo`（请求体）由 `data` 单独接收，**不进 params interface 的 extends**；
    /// 只有 `dot`（query 引用模型）才合并进 params。当 params 仅由单个 dot 或单个 Paging
    /// 构成时，直接用其类型作签名，无需生成具名 interface。
    fn params_interface(&self, ep: &IrEndpoint, iface_name: &str, is_paging: bool) -> Option<String> {
        match self.params_kind(ep, is_paging) {
            ParamsArg::None | ParamsArg::DotDirect | ParamsArg::PagingDirect => None,
            ParamsArg::Interface => {
                let dot = ep.query_ref_param();
                // extends 只含 dot（query 引用模型）；vo 由 data 传，不在此
                let mut extends = match dot {
                    Some(d) => format!("extends {}", self.map_type(&d.ty)),
                    None => String::new(),
                };
                if is_paging {
                    extends = if extends.is_empty() {
                        "extends Paging".into()
                    } else {
                        format!("{extends},Paging")
                    };
                }

                let mut body = String::new();
                for p in ep.scalar_params() {
                    body.push_str(&format!("  {}?: {};\n", p.name, self.map_type(&p.ty)));
                }
                if ep.has_file() {
                    body.push_str("  [key:string]: any;\n");
                }

                let space = if extends.is_empty() { "" } else { " " };
                Some(format!("export interface {iface_name}{space}{extends} {{\n{body}}}"))
            }
        }
    }

    /// 函数签名参数（§7.1 paramsD）。
    fn params_decl(&self, ep: &IrEndpoint, iface_name: &str, is_paging: bool) -> String {
        let mut parts = Vec::new();
        match self.params_kind(ep, is_paging) {
            ParamsArg::None => {}
            ParamsArg::DotDirect => {
                let dot = ep.query_ref_param().unwrap();
                parts.push(format!("params?:{}", self.map_type(&dot.ty)));
            }
            ParamsArg::PagingDirect => parts.push("params?:Paging".to_string()),
            ParamsArg::Interface => parts.push(format!("params?:{iface_name}")),
        }
        if let Some(b) = ep.body_param() {
            parts.push(format!("data?:{}", self.map_type(&b.ty)));
        }
        parts.join(",")
    }

    /// 判断 params 实参形态（interface / 仅dot / 仅Paging / 无）。
    ///
    /// `file` 参数也需要 params（函数体遍历 params 构造 FormData），故计入。
    fn params_kind(&self, ep: &IrEndpoint, is_paging: bool) -> ParamsArg {
        let has_scalar = ep.scalar_params().next().is_some();
        let has_file = ep.has_file();
        let has_dot = ep.query_ref_param().is_some();

        if !has_scalar && !has_file && !has_dot && !is_paging {
            return ParamsArg::None;
        }
        // 仅单个 dot（无 scalar/file/分页）→ 直接用 dot 类型
        if has_dot && !has_scalar && !has_file && !is_paging {
            return ParamsArg::DotDirect;
        }
        // 仅分页（无 scalar/dot/file）→ 直接用 Paging
        if is_paging && !has_scalar && !has_dot && !has_file {
            return ParamsArg::PagingDirect;
        }
        ParamsArg::Interface
    }

    /// axios 第二参数 config（§7.1 axiosConfig）。
    fn axios_config(
        &self,
        has_file: bool,
        has_body: bool,
        scalars: &[&IrParam],
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
        // 只有「真正的 query 参数」才需要传 params：path 参数已在 URL 模板里插值用掉，
        // 不应再作为 query 重复传递（否则会生成多余的 {params}）。
        let has_query_scalar = scalars.iter().any(|p| !p.in_path);
        if !has_file && (has_query_scalar || has_dot || is_paging) {
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
    return res as ArrayBuffer;
  } else {
    return null;
  }
"#;

/// `/user/{id}` → `/user/${params?.id}`（§6 TS url 模板）。
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

/// `get_user_info` → `GetUserInfo`（每段首字母大写其余小写，§6）。
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
    use crate::ir::{IrField, IrModel, ParamKind};
    use crate::openapi::HttpMethod;

    fn ts() -> TypescriptGen {
        TypescriptGen
    }

    #[test]
    fn maps_types() {
        let g = ts();
        assert_eq!(g.map_type(&IrType::Long), "string");
        assert_eq!(g.map_type(&IrType::Int), "number");
        assert_eq!(g.map_type(&IrType::Array(Box::new(IrType::Ref("Vo".into())))), "Vo[]");
        assert_eq!(g.map_type(&IrType::Map(Box::new(IrType::String))), "{[key:string]:string}");
        assert_eq!(g.map_type(&IrType::IPage(Box::new(IrType::Ref("Vo".into())))), "IPage<Vo>");
        assert_eq!(g.map_type(&IrType::Binary), "ArrayBuffer");
    }

    #[test]
    fn gen_model_with_enum_hint() {
        let g = ts();
        let m = IrModel {
            name: "AppLoginDto".into(),
            fields: vec![
                IrField { name: "id".into(), ty: IrType::Long, description: None },
                IrField { name: "status".into(), ty: IrType::MsgType, description: Some("状态枚举".into()) },
            ],
        };
        let out = g.gen_model(&m);
        assert!(out.contains("export interface AppLoginDto {"));
        assert!(out.contains("id?: string;"));
        // 描述含「枚举」→ number
        assert!(out.contains("/** 状态枚举 */"));
        assert!(out.contains("status?: number;"));
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
    fn simple_get_no_params() {
        let g = ts();
        let e = ep("get_user_info", HttpMethod::Get, "/user/info", vec![], IrType::Ref("UserVo".into()), false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("export const get_user_info = async () =>"));
        assert!(out.contains("server.GET<UserVo> (`/user/info`)"));
        assert!(out.contains("return res?.result;"));
    }

    #[test]
    fn path_param_url_template() {
        let g = ts();
        let p = IrParam { name: "id".into(), ty: IrType::Long, kind: ParamKind::Scalar, in_path: true };
        let e = ep("get_user_id", HttpMethod::Get, "/user/{id}", vec![p], IrType::Bool, false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("`/user/${params?.id}`"), "{out}");
        // 签名仍需 params（URL 插值要用），但…
        assert!(out.contains("params?:GetUserId"));
        // …唯一参数是 path 参数，不应再传多余的 query {params}
        assert!(!out.contains("{params}"), "path-only 不应有多余 {{params}}: {out}");
        assert!(out.contains("`/user/${params?.id}`);"), "{out}");
    }

    #[test]
    fn mixed_path_and_query_still_passes_params() {
        let g = ts();
        let id = IrParam { name: "id".into(), ty: IrType::Long, kind: ParamKind::Scalar, in_path: true };
        let kw = IrParam { name: "keyword".into(), ty: IrType::String, kind: ParamKind::Scalar, in_path: false };
        let e = ep("get_user_id_search", HttpMethod::Get, "/user/{id}/search", vec![id, kw], IrType::Bool, false);
        let out = g.gen_endpoint(&e);
        // 存在真正的 query 参数 keyword → 仍传 {params}
        assert!(out.contains(", {params}"), "{out}");
        assert!(out.contains("`/user/${params?.id}/search`"));
    }

    #[test]
    fn body_param_data() {
        let g = ts();
        let p = IrParam { name: "vo".into(), ty: IrType::Ref("LoginDto".into()), kind: ParamKind::Body, in_path: false };
        let e = ep("post_login", HttpMethod::Post, "/login", vec![p], IrType::Ref("UserVo".into()), false);
        let out = g.gen_endpoint(&e);
        // 单个 vo 不生成 interface，直接 data?:LoginDto
        assert!(!out.contains("export interface PostLogin"));
        assert!(out.contains("data?:LoginDto"));
        assert!(out.contains("server.POST<UserVo>"));
        assert!(out.contains(", {data}"));
    }

    #[test]
    fn param_iface_avoids_model_name_collision() {
        use crate::ir::IrModule;
        let g = ts();
        // 模型名恰好等于某接口的 PascalCase 函数名 → 参数 interface 需加 Params 后缀
        let p = IrParam { name: "id".into(), ty: IrType::Long, kind: ParamKind::Scalar, in_path: false };
        let e = ep("get_user_info", HttpMethod::Get, "/user/info", vec![p], IrType::Bool, false);
        let module = IrModule {
            endpoints: vec![e],
            models: vec![IrModel { name: "GetUserInfo".into(), fields: vec![] }],
        };
        let out = g.generate(&module);
        // 模型 interface 与参数 interface 不应同名
        assert!(out.contains("export interface GetUserInfoParams"), "{out}");
        assert!(out.contains("params?:GetUserInfoParams"));
        assert!(out.contains("export interface GetUserInfo {"));
    }

    #[test]
    fn path_param_with_body_no_extends() {
        // 用户报告的 case：同时有 path 参数(id) 和 body(vo)
        let g = ts();
        let id = IrParam { name: "id".into(), ty: IrType::Long, kind: ParamKind::Scalar, in_path: true };
        let vo = IrParam { name: "vo".into(), ty: IrType::Ref("OrderCommentDto".into()), kind: ParamKind::Body, in_path: false };
        let e = ep("post_orders_id_comment", HttpMethod::Post, "/orders/{id}/comment", vec![id, vo], IrType::Void, false);
        let out = g.gen_endpoint(&e);
        // interface 只含 path 参数，不再 extends body 模型
        assert!(out.contains("export interface PostOrdersIdComment {"), "{out}");
        assert!(!out.contains("extends OrderCommentDto"), "不应继承 body 模型: {out}");
        assert!(out.contains("id?: string;"));
        // body 由 data 接收
        assert!(out.contains("params?:PostOrdersIdComment,data?:OrderCommentDto"), "{out}");
        // path 参数不传 query，body 走 data
        assert!(out.contains(", {data}"));
        assert!(out.contains("`/orders/${params?.id}/comment`"));
    }

    #[test]
    fn export_returns_arraybuffer() {
        let g = ts();
        let e = ep("get_order_export", HttpMethod::Get, "/order/export", vec![], IrType::Binary, true);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("server.GET<ArrayBuffer>"));
        assert!(out.contains("responseType: 'arraybuffer'"));
        assert!(out.contains("return res as ArrayBuffer;"));
    }

    #[test]
    fn array_result_appends_default() {
        let g = ts();
        let e = ep("get_list", HttpMethod::Get, "/list", vec![], IrType::Array(Box::new(IrType::Ref("Vo".into()))), false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("server.GET<Vo[]>"));
        assert!(out.contains("return res?.result ?? [];"));
    }

    #[test]
    fn void_result_returns_success() {
        let g = ts();
        let e = ep("post_do", HttpMethod::Post, "/do", vec![], IrType::Void, false);
        let out = g.gen_endpoint(&e);
        assert!(!out.contains("server.POST<"));
        assert!(out.contains("return res?.success;"));
    }

    #[test]
    fn paging_extends_paging() {
        let g = ts();
        let p = IrParam { name: "keyword".into(), ty: IrType::String, kind: ParamKind::Scalar, in_path: false };
        let e = ep("get_page", HttpMethod::Get, "/page", vec![p], IrType::IPage(Box::new(IrType::Ref("Vo".into()))), false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("export interface GetPage extends Paging {"), "{out}");
        assert!(out.contains("keyword?: string;"));
        assert!(out.contains("server.GET<IPage<Vo>>"));
    }
}
