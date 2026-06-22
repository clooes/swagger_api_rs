//! 接口参数解析（MIGRATION.md §5.5，对应两份 `schemaParamsType`）。

use super::field_type::lower_type;
use crate::ir::{IrParam, IrType, ParamKind};
use crate::openapi::{Operation, Schema};

/// 解析一个操作的全部参数（query/path 标量、query 引用模型、JSON body、文件上传）。
pub fn lower_params(op: &Operation) -> Vec<IrParam> {
    let mut params = Vec::new();

    // operation.parameters：标量参数或 query 引用模型
    for p in &op.parameters {
        if let Some(r) = p.schema.ref_name() {
            // 引用模型整体作为 query 参数（原版 name=="dot"）
            params.push(IrParam {
                name: "dot".to_string(),
                ty: IrType::Ref(r.to_string()),
                kind: ParamKind::QueryRef,
                in_path: false,
            });
        } else {
            // 标量参数。原版只取 type+format（Flutter 的 format 回退到 items.format），不传 items。
            let synthetic = Schema {
                schema_type: p.schema.schema_type.clone(),
                format: p
                    .schema
                    .format
                    .clone()
                    .or_else(|| p.schema.items.as_deref().and_then(|i| i.format.clone())),
                ..Default::default()
            };
            params.push(IrParam {
                name: p.name.clone(),
                ty: lower_type(&synthetic, true),
                kind: ParamKind::Scalar,
                in_path: false, // 由 lower/mod 按 url 模板回填
            });
        }
    }

    // requestBody：JSON body 模型 或 文件上传
    if let Some(rb) = &op.request_body {
        match rb.content.get("application/json").and_then(|m| m.schema.as_ref()) {
            Some(schema) if schema.ref_name().is_some() => {
                let name = schema.ref_name().unwrap();
                // 特例：LongList 请求体视为 string[]（§5.5）
                let ty = if name == "LongList" {
                    IrType::Array(Box::new(IrType::String))
                } else {
                    IrType::Ref(name.to_string())
                };
                params.push(IrParam {
                    name: "vo".to_string(),
                    ty,
                    kind: ParamKind::Body,
                    in_path: false,
                });
            }
            // 无 application/json $ref（如 multipart）→ 文件上传
            _ => {
                params.push(IrParam {
                    name: "file".to_string(),
                    ty: IrType::File,
                    kind: ParamKind::File,
                    in_path: false,
                });
            }
        }
    }

    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(json: &str) -> Operation {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn scalar_and_ref_params() {
        let o = op(r##"{"parameters":[
            {"name":"id","schema":{"type":"integer","format":"int64"}},
            {"name":"q","schema":{"$ref":"#/c/QueryDto"}}
        ]}"##);
        let ps = lower_params(&o);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].name, "id");
        assert_eq!(ps[0].ty, IrType::Long);
        assert_eq!(ps[0].kind, ParamKind::Scalar);
        assert_eq!(ps[1].kind, ParamKind::QueryRef);
        assert_eq!(ps[1].ty, IrType::Ref("QueryDto".into()));
    }

    #[test]
    fn body_param() {
        let o = op(r##"{"requestBody":{"content":{"application/json":{"schema":{"$ref":"#/c/LoginDto"}}}}}"##);
        let ps = lower_params(&o);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].kind, ParamKind::Body);
        assert_eq!(ps[0].ty, IrType::Ref("LoginDto".into()));
    }

    #[test]
    fn longlist_body_is_string_array() {
        let o = op(r##"{"requestBody":{"content":{"application/json":{"schema":{"$ref":"#/c/LongList"}}}}}"##);
        let ps = lower_params(&o);
        assert_eq!(ps[0].ty, IrType::Array(Box::new(IrType::String)));
        assert_eq!(ps[0].kind, ParamKind::Body);
    }

    #[test]
    fn multipart_is_file() {
        let o = op(r#"{"requestBody":{"content":{"multipart/form-data":{"schema":{"type":"object"}}}}}"#);
        let ps = lower_params(&o);
        assert_eq!(ps[0].kind, ParamKind::File);
        assert_eq!(ps[0].ty, IrType::File);
    }
}
