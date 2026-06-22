//! Flutter/Dart 代码生成器（对应原 splice.flutter.js + const.js FlutterOtherType）。
//!
//! 仅做「IR → Dart 语法」翻译。Flutter 把每个标量参数作为独立的具名参数，
//! 没有 TS 那种 params interface，故 vo-extends 问题不存在；但 path 参数不应
//! 重复进 dio 的 `params:{}`（§13 #7），这里用 `in_path` 排除。

use super::CodeGenerator;
use crate::ir::{IrEndpoint, IrField, IrModel, IrType};

/// Flutter/Dart 后端。
pub struct FlutterGen;

/// 内置通用类型（const.js FlutterOtherType）。
const BUILTIN: &str = r#"class IPage<T> {
  int? pages;
  List<T>? records;
  int? total;
  int? size;
  int? current;

  IPage({this.pages, this.records, this.total, this.size, this.current});

  IPage.fromJson(
      Map<String, dynamic> json, Function(Map<String, dynamic>) fromJsonModel) {
    pages = json['pages'];
    if (json['records'] != null) {
      records = <T>[];
      json['records'].forEach((v) {
        records?.add(fromJsonModel(v));
      });
    }
    total = json['total'];
    size = json['size'];
    current = json['current'];
  }
}

class MsgType {
  String? node;
  int? type;
  bool? status;
  bool? free;
  bool? ppl;
  bool? special;
  int? value;

  MsgType({this.node, this.type, this.free, this.value, this.ppl, this.special});

  MsgType.fromJson(Map<String, dynamic> json) {
    node = json['node'];
    if (json['type'] is int) {
      type = json['type'];
    }
    if (json['type'] is bool) {
      status = json['type'];
    }
  }

  Map<String, dynamic> toJson() {
    final Map<String, dynamic> data = <String, dynamic>{};
    data['node'] = node;
    data['type'] = type ?? status;
    return data;
  }
}
"#;

impl CodeGenerator for FlutterGen {
    fn map_type(&self, ty: &IrType) -> String {
        match ty {
            IrType::Int => "int".into(),
            IrType::Long => "String".into(),
            IrType::Double => "double".into(),
            IrType::Bool => "bool".into(),
            IrType::String => "String".into(),
            IrType::Array(inner) => format!("List<{}>", self.map_type(inner)),
            IrType::Map(inner) => format!("Map<String,{}>", self.map_type(inner)),
            IrType::Ref(name) => name.clone(),
            IrType::IPage(inner) => format!("IPage<{}>", self.map_type(inner)),
            IrType::MsgType => "MsgType".into(),
            IrType::File => "XFile".into(),
            IrType::Binary => "ArrayBuffer".into(),
            IrType::Any => "dynamic".into(),
            IrType::Void => "void".into(),
        }
    }

    fn gen_model(&self, model: &IrModel) -> String {
        self.gen_model_impl(model)
    }

    fn gen_endpoint(&self, ep: &IrEndpoint) -> String {
        self.gen_endpoint_impl(ep)
    }

    fn builtin_types(&self) -> &'static str {
        BUILTIN
    }

    fn file_ext(&self) -> &'static str {
        "dart"
    }
}

/// 单个 url 参数（标量 / 文件 / 分页），承载生成所需信息。
struct UrlParam {
    name: String,
    dart: String,
    in_path: bool,
    is_file: bool,
}

impl FlutterGen {
    /// 字段 Dart 类型：描述含「枚举」→ int（§7.2）。
    fn field_type(&self, f: &IrField) -> String {
        if f.is_enum_hint() {
            "int".into()
        } else {
            self.map_type(&f.ty)
        }
    }

    fn gen_model_impl(&self, model: &IrModel) -> String {
        let name = &model.name;
        let fields: Vec<(String, String, Option<String>)> = model
            .fields
            .iter()
            .map(|f| (f.name.clone(), self.field_type(f), f.description.clone()))
            .collect();

        let mut out = format!("class {name} {{\n");
        // 字段声明
        for (key, dt, desc) in &fields {
            if let Some(d) = desc {
                out.push_str(&format!("  /// {d}\n"));
            }
            out.push_str(&format!("  {dt}? {key};\n"));
        }
        out.push('\n');

        // toJson
        out.push_str("  Map<String, dynamic> toJson() {\n");
        out.push_str("    final Map<String, dynamic> data = <String, dynamic>{};\n");
        for (key, dt, _) in &fields {
            out.push_str(&self.to_json_line(key, dt));
        }
        out.push_str("    return data;\n  }\n\n");

        // 构造函数
        let ctor_args: Vec<String> = fields.iter().map(|(k, _, _)| format!("this.{k}")).collect();
        out.push_str(&format!("  {name}({{{}}});\n\n", ctor_args.join(", ")));

        // fromJson
        out.push_str(&format!("  {name}.fromJson(Map<String, dynamic> json) {{\n"));
        for (key, dt, _) in &fields {
            out.push_str(&self.from_json_line(key, dt));
        }
        out.push_str("  }\n}\n");
        out
    }

    fn to_json_line(&self, key: &str, dt: &str) -> String {
        if is_dart_primitive(dt) {
            format!("    data['{key}'] = {key};\n")
        } else if let Some(inner) = list_inner(dt) {
            if is_dart_primitive(inner) {
                format!("    data['{key}'] = {key};\n")
            } else {
                format!(
                    "    if ({key} != null) {{\n      data['{key}'] = {key}?.map((v) => v.toJson()).toList();\n    }}\n"
                )
            }
        } else {
            format!("    data['{key}'] = {key}?.toJson();\n")
        }
    }

    fn from_json_line(&self, key: &str, dt: &str) -> String {
        if is_dart_primitive(dt) {
            format!("    {key} = json['{key}'];\n")
        } else if let Some(inner) = list_inner(dt) {
            if is_dart_primitive(inner) {
                format!("    {key} = json['{key}']?.cast<{inner}>();\n")
            } else {
                format!(
                    "    if (json['{key}'] != null) {{\n      {key} = [];\n      json['{key}'].forEach((v) {{\n        {key}?.add({inner}.fromJson(v));\n      }});\n    }}\n"
                )
            }
        } else {
            format!(
                "    if (json['{key}'] != null) {{\n      {key} = {dt}.fromJson(json['{key}']);\n    }}\n"
            )
        }
    }

    fn gen_endpoint_impl(&self, ep: &IrEndpoint) -> String {
        let dot = ep.query_ref_param();
        let vo = ep.body_param();
        let is_paging = ep.is_paging();

        // 返回类型：导出 → ArrayBuffer；Void → None；否则 map_type
        let result_str: Option<String> = if ep.is_export {
            Some("ArrayBuffer".into())
        } else if ep.result.is_void() {
            None
        } else {
            Some(self.map_type(&ep.result))
        };
        let is_result_list = ep.result.is_array();

        // url 参数 = 标量 + 文件参数（+ 分页 pageNum/pageSize）
        let mut url_params: Vec<UrlParam> = Vec::new();
        for p in ep.scalar_params() {
            url_params.push(UrlParam {
                name: p.name.clone(),
                dart: self.map_type(&p.ty),
                in_path: p.in_path,
                is_file: is_file_type(&p.ty),
            });
        }
        for p in ep.params.iter().filter(|p| p.kind == crate::ir::ParamKind::File) {
            url_params.push(UrlParam {
                name: p.name.clone(),
                dart: self.map_type(&p.ty),
                in_path: false,
                is_file: true,
            });
        }
        if is_paging {
            url_params.push(UrlParam { name: "pageNum".into(), dart: "int".into(), in_path: false, is_file: false });
            url_params.push(UrlParam { name: "pageSize".into(), dart: "int".into(), in_path: false, is_file: false });
        }
        let has_file = url_params.iter().any(|u| u.is_file);

        // 具名参数：dot → params，各 url 参数，vo → data
        let mut named = Vec::new();
        if let Some(d) = dot {
            named.push(format!("required {} params", self.map_type(&d.ty)));
        }
        for u in &url_params {
            named.push(format!("{}? {}", u.dart, u.name));
        }
        if let Some(v) = vo {
            named.push(format!("required {} data", self.map_type(&v.ty)));
        }
        let named_clause = if named.is_empty() {
            String::new()
        } else {
            format!("{{{}}}", named.join(", "))
        };

        // dio params:{} —— 排除 path 参数与文件参数；dot 展开
        let mut dio_params = Vec::new();
        if dot.is_some() {
            dio_params.push("...params.toJson()".to_string());
        }
        if !has_file {
            for u in &url_params {
                if !u.in_path && !u.is_file {
                    dio_params.push(format!("\"{}\":{}", u.name, u.name));
                }
            }
        }
        let dio_params_str = dio_params.join(",");

        // 文件上传 FormData 片段
        let file_str = if has_file {
            self.build_form_data(&url_params, dot.is_some(), vo.is_some())
        } else {
            String::new()
        };

        // data: 子句
        let data_clause = if has_file {
            ",\n        data: formData".to_string()
        } else if vo.is_some() {
            ",\n        data: data.toJson()".to_string()
        } else {
            String::new()
        };

        // fromJson 回调
        let from_json = self.result_from_json(&ep.result);
        let from_json_clause = match &from_json {
            Some(body) => format!(",\n        fromJson: (data) {{\n          {body}\n        }}"),
            None => String::new(),
        };

        // 组装
        let future_ty = match &result_str {
            Some(t) => format!("{t}{}", if is_result_list { "" } else { "?" }),
            None => "bool".to_string(),
        };
        let generic = result_str.as_deref().unwrap_or("void");
        let ret = if result_str.is_some() { "result" } else { "success" };
        let ret_default = if is_result_list { " ?? []" } else { "" };

        let mut out = String::new();
        out.push_str(&format!("/// {}\n", ep.summary.as_deref().unwrap_or("")));
        out.push_str(&format!(
            "Future<{}> {}({}) async {{\n",
            future_ty,
            camel_case(&ep.func_name),
            named_clause
        ));
        if !file_str.is_empty() {
            out.push_str(&file_str);
        }
        out.push_str(&format!(
            "  var res = await DioUtil.instance.request<{}>(\"{}\",\n        method: DioMethod.{}, params: {{{}}}{}{});\n",
            generic,
            url_template(&ep.url),
            ep.http_method.as_lower(),
            dio_params_str,
            data_clause,
            from_json_clause,
        ));
        out.push_str(&format!("  return res.{ret}{ret_default};\n}}\n"));
        out
    }

    /// 生成 FormData.fromMap 片段（含 MultipartFile 转换）。
    fn build_form_data(&self, url_params: &[UrlParam], has_dot: bool, has_vo: bool) -> String {
        let Some(file) = url_params.iter().find(|u| u.is_file) else {
            return String::new();
        };
        let name = &file.name;
        let dp = if file.dart == "List<XFile>" {
            format!(
                "  List<MultipartFile> fd = [];\n  for (var element in {name} ?? []) {{\n    fd.add(await MultipartFile.fromFile(element.path));\n  }}\n"
            )
        } else {
            format!(
                "  MultipartFile? fd;\n  if ({name} != null) {{\n    fd = await MultipartFile.fromFile({name}.path);\n  }}\n"
            )
        };

        let mut entries = vec![format!("\"{name}\": fd")];
        for u in url_params {
            if !u.is_file {
                entries.push(format!("\"{}\": {}", u.name, u.name));
            }
        }
        let mut spread = String::new();
        if has_dot {
            spread.push_str("\n    ...params.toJson(),");
        }
        if has_vo {
            spread.push_str("\n    ...data.toJson(),");
        }
        format!(
            "{dp}  FormData formData = FormData.fromMap({{\n    {}{}\n  }});\n",
            entries.join(",\n    "),
            spread
        )
    }

    /// 返回类型对应的 fromJson 回调体（None 表示无需 fromJson）。
    fn result_from_json(&self, ty: &IrType) -> Option<String> {
        match ty {
            // 基础类型 / 无需反序列化
            IrType::Void | IrType::Int | IrType::Long | IrType::Double | IrType::Bool
            | IrType::String | IrType::Any | IrType::Binary | IrType::Map(_) => None,
            IrType::Array(inner) => {
                let inner_dart = self.map_type(inner);
                if is_dart_primitive(&inner_dart) {
                    Some(format!("return data?.cast<{inner_dart}>();"))
                } else {
                    Some(format!(
                        "List<{inner_dart}> list = [];\n          if (data != null) {{\n            for (var item in data) {{\n              list.add({inner_dart}.fromJson(item));\n            }}\n          }}\n          return list;"
                    ))
                }
            }
            IrType::IPage(inner) => {
                let inner_dart = self.map_type(inner);
                Some(format!(
                    "return IPage<{inner_dart}>.fromJson(data, {inner_dart}.fromJson);"
                ))
            }
            IrType::Ref(name) => Some(format!("return {name}.fromJson(data);")),
            IrType::MsgType => Some("return MsgType.fromJson(data);".to_string()),
            IrType::File => None,
        }
    }
}

/// Dart 基础类型（无 fromJson/toJson）。
fn is_dart_primitive(dt: &str) -> bool {
    matches!(dt, "int" | "double" | "bool" | "String" | "dynamic" | "num")
}

/// 若是 `List<X>` 返回内层 X。
fn list_inner(dt: &str) -> Option<&str> {
    dt.strip_prefix("List<").and_then(|s| s.strip_suffix('>'))
}

/// IrType 是否为文件（File 或 List<File>）。
fn is_file_type(ty: &IrType) -> bool {
    matches!(ty, IrType::File) || matches!(ty, IrType::Array(inner) if matches!(**inner, IrType::File))
}

/// `/orders/{id}/comment` → `/orders/$id/comment`（Dart 字符串插值，§6）。
fn url_template(url: &str) -> String {
    let mut out = String::new();
    let mut chars = url.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            out.push('$');
            for c2 in chars.by_ref() {
                if c2 == '}' {
                    break;
                }
                out.push(c2);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `get_orders_id` → `getOrdersId`（首段保留，其余首字母大写，§6）。
fn camel_case(s: &str) -> String {
    let mut out = String::new();
    for (i, seg) in s.split('_').filter(|x| !x.is_empty()).enumerate() {
        if i == 0 {
            out.push_str(seg);
        } else {
            let mut cs = seg.chars();
            if let Some(first) = cs.next() {
                out.push(first.to_ascii_uppercase());
                out.push_str(cs.as_str());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrField, IrModel, IrParam, ParamKind};
    use crate::openapi::HttpMethod;

    fn fl() -> FlutterGen {
        FlutterGen
    }

    #[test]
    fn maps_types() {
        let g = fl();
        assert_eq!(g.map_type(&IrType::Long), "String");
        assert_eq!(g.map_type(&IrType::Int), "int");
        assert_eq!(g.map_type(&IrType::Array(Box::new(IrType::Ref("Vo".into())))), "List<Vo>");
        assert_eq!(g.map_type(&IrType::IPage(Box::new(IrType::Ref("Vo".into())))), "IPage<Vo>");
        assert_eq!(g.map_type(&IrType::File), "XFile");
    }

    #[test]
    fn model_with_class_and_json() {
        let g = fl();
        let m = IrModel {
            name: "AppLoginDto".into(),
            fields: vec![
                IrField { name: "phone".into(), ty: IrType::String, description: Some("电话".into()) },
                IrField { name: "tags".into(), ty: IrType::Array(Box::new(IrType::Ref("Tag".into()))), description: None },
            ],
        };
        let out = g.gen_model(&m);
        assert!(out.contains("class AppLoginDto {"));
        assert!(out.contains("/// 电话"));
        assert!(out.contains("String? phone;"));
        assert!(out.contains("List<Tag>? tags;"));
        assert!(out.contains("AppLoginDto({this.phone, this.tags});"));
        assert!(out.contains("AppLoginDto.fromJson(Map<String, dynamic> json)"));
        // 对象列表 fromJson
        assert!(out.contains("tags?.add(Tag.fromJson(v));"));
        // 基础类型
        assert!(out.contains("phone = json['phone'];"));
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
    fn simple_get_object_result() {
        let g = fl();
        let e = ep("get_user_info", HttpMethod::Get, "/user/info", vec![], IrType::Ref("UserVo".into()), false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("Future<UserVo?> getUserInfo() async {"), "{out}");
        assert!(out.contains("DioUtil.instance.request<UserVo>(\"/user/info\""));
        assert!(out.contains("return UserVo.fromJson(data);"));
        assert!(out.contains("return res.result;"));
    }

    #[test]
    fn path_param_excluded_from_query() {
        let g = fl();
        let id = IrParam { name: "id".into(), ty: IrType::Long, kind: ParamKind::Scalar, in_path: true };
        let e = ep("get_user_id", HttpMethod::Get, "/user/{id}", vec![id], IrType::Bool, false);
        let out = g.gen_endpoint(&e);
        // url 插值用 $id
        assert!(out.contains("request<bool>(\"/user/$id\""), "{out}");
        // 但不进 dio params
        assert!(out.contains("params: {}"), "path 参数不应进 query: {out}");
        // 仍是具名参数
        assert!(out.contains("String? id"));
    }

    #[test]
    fn body_param_data() {
        let g = fl();
        let vo = IrParam { name: "vo".into(), ty: IrType::Ref("LoginDto".into()), kind: ParamKind::Body, in_path: false };
        let e = ep("post_login", HttpMethod::Post, "/login", vec![vo], IrType::Ref("UserVo".into()), false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("required LoginDto data"));
        assert!(out.contains("data: data.toJson()"));
    }

    #[test]
    fn list_result_appends_default() {
        let g = fl();
        let e = ep("get_list", HttpMethod::Get, "/list", vec![], IrType::Array(Box::new(IrType::Ref("Vo".into()))), false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("Future<List<Vo>> getList() async {"), "{out}");
        assert!(out.contains("list.add(Vo.fromJson(item));"));
        assert!(out.contains("return res.result ?? [];"));
    }

    #[test]
    fn paging_adds_page_params() {
        let g = fl();
        let e = ep("get_page", HttpMethod::Get, "/page", vec![], IrType::IPage(Box::new(IrType::Ref("Vo".into()))), false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("int? pageNum"));
        assert!(out.contains("int? pageSize"));
        assert!(out.contains("IPage<Vo>.fromJson(data, Vo.fromJson);"));
        assert!(out.contains("\"pageNum\":pageNum"));
    }

    #[test]
    fn void_returns_success_bool() {
        let g = fl();
        let e = ep("post_do", HttpMethod::Post, "/do", vec![], IrType::Void, false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("Future<bool> postDo() async {"), "{out}");
        assert!(out.contains("request<void>(\"/do\""));
        assert!(out.contains("return res.success;"));
    }

    #[test]
    fn file_upload_formdata() {
        let g = fl();
        let file = IrParam { name: "file".into(), ty: IrType::File, kind: ParamKind::File, in_path: false };
        let e = ep("post_upload", HttpMethod::Post, "/upload", vec![file], IrType::Bool, false);
        let out = g.gen_endpoint(&e);
        assert!(out.contains("MultipartFile? fd;"), "{out}");
        assert!(out.contains("MultipartFile.fromFile(file.path)"));
        assert!(out.contains("FormData formData = FormData.fromMap("));
        assert!(out.contains("data: formData"));
    }
}
