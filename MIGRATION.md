# Swagger API 代码生成器 —— Rust 重构迁移文档

> 本文档用于指导将现有 JS 工具（`swagger-api-ts`）重构为 Rust 版本。
> 目标：**Swagger/OpenAPI JSON → 语言无关 IR/AST → 多语言代码（TypeScript / JavaScript / Flutter）**。
> 后续 AI 应以本文档为唯一事实来源（Single Source of Truth），逐步实现 TODO 列表。

---

## 0. 背景与目标

### 现状（JS 版本）
现有工具直接「字符串拼接」生成代码，TypeScript 与 Flutter **各写一套几乎重复的逻辑**；类型映射规则（`R<T>` 解包、`MapString`、`IPage<T>`、`int64→string`、文件上传等）**散落在多个文件中**，新增语言要从头复制粘贴。

### 重构目标（Rust 版本）
采用**编译器式三段架构**：

```
Swagger JSON ──(Frontend 解析)──> OpenAPI Spec 模型
                                        │
                                  (Lowering ★)   ← 所有类型映射规则收敛于此，只做一次
                                        ▼
                                  IR / AST (语言无关)
                                        │
                              (Backend Codegen ★)  ← 每语言一个，仅做「IR→语法」纯翻译
                          ┌─────────────┼─────────────┐
                          ▼             ▼             ▼
                     TypeScript    JavaScript      Flutter
```

**核心原则**：
1. 所有「Java 类型名 → 通用语义」的解析逻辑，**只在 `lower/` 写一次**。
2. IR 必须**完全语言无关**（不出现 `number`/`int`/`String` 这类目标语言词汇）。
3. 各语言后端实现统一 `CodeGenerator` trait，仅负责「IR 节点 → 目标语法字符串」。
4. 新增语言 = 新增一个 trait 实现，**不改动 frontend / lowering**。

---

## 1. 现有 JS 代码结构与职责

| 文件 | 行数 | 职责 | 对应 Rust 模块 |
|------|------|------|----------------|
| `src/index.js` | 57 | 入口；读配置→HTTP 拉 `swagger-config`→拉各 `api-docs` JSON | `main.rs` + `fetcher/` |
| `src/tools.js` | 41 | 配置文件读取 + 命令行参数解析 | `config.rs` + `cli.rs` |
| `src/analyze.js` | 102 | 遍历 `paths` 与 `components.schemas`，按语言分发，写文件 | `lower/` + `emit.rs` |
| `src/const.js` | 91 | 通用类型模板（IPage / MsgType / Paging） | 各 codegen 的 `builtin_types()` |
| `src/splice.js` | 444 | **TypeScript** 代码生成 | `codegen/typescript.rs` + 类型规则进 `lower/` |
| `src/splice.flutter.js` | 567 | **Flutter/Dart** 代码生成 | `codegen/flutter.rs` + 类型规则进 `lower/` |

---

## 2. 数据获取流程（对应 index.js）

1. 读配置文件 `swagger.json`（或 `swagger.{file}.json`，由 `--file=xxx` 指定）。
2. 拼接 URL：`{url}{suffix}/v3/api-docs/swagger-config`，HTTP GET。
3. 响应 JSON：
   - 若有 `urls` 字段（数组，元素含 `.url`）→ 遍历每个 `url`，逐个 `GET {base_url}{url}` 拉取分组 api-docs。
   - 否则若有 `url` 字段 → 直接拉这一个。
   - 都没有 → 报错「没有地址」。
4. 每个 api-docs JSON 交给 analyze 处理。
5. 输出目录在开始时清空（`fs.emptyDirSync`）。

**Rust 注意**：原 JS 用回调式 `http.get`，Rust 用 `reqwest`（建议 blocking 或 tokio 均可）。`process.env.PWD` → `std::env::current_dir()`。建议额外支持「本地 JSON 文件输入」便于测试（可选增强）。

---

## 3. 配置文件结构（对应 tools.js）

```jsonc
{
  "url": "http://dpa.host",        // 必填，接口根地址
  "suffix": "",                     // 可选，接口后缀（如 "/api-patrol"）
  "output": "src/api",              // 必填，输出目录（相对当前工作目录）
  "language": "flutter",            // 可选，"flutter" | "js"；默认走 ts 分支
  "deprecated": false,              // 可选，是否生成已废弃接口；默认 falsy
  "header": [                       // 代码头部注入（import 语句等），按 \n join
    "import { server } from '@/utils/axios/request';"
  ],
  "filter": ["/common/oss/ali"]     // 可选，按「path key」精确过滤掉的接口
}
```

- 命令行参数格式：`--file=xxx` → 读取 `swagger.xxx.json`。解析逻辑见 `tools.js getNodeParams`：取形如 `--key=value` 的参数。
- 校验：`url` 为空报错；`output` 为空报错。

**Rust language 枚举建议**（注意比原版更明确）：
```rust
enum Language { Typescript, Javascript, Flutter }
```
原 JS 只有 `flutter` 和「默认(ts)」两个分支，`js` 是本次新增目标。配置里 `"language"` 取值映射：`"flutter"→Flutter`、`"ts"|缺省→Typescript`、`"js"→Javascript`。

---

## 4. OpenAPI Spec 数据模型（serde 反序列化）

需要从 api-docs JSON 反序列化的字段（OpenAPI 3 风格）：

```
root
├── paths: Map<String /*url*/, PathItem>
│   └── PathItem: Map<String /*"get"|"post"|"put"|"delete"*/, Operation>
│       └── Operation
│           ├── summary: Option<String>
│           ├── deprecated: Option<bool>
│           ├── parameters: Option<Vec<Parameter>>
│           │   └── Parameter { name, schema: Schema /* 可能含 $ref 或 type/format/items */ }
│           ├── requestBody: Option<RequestBody>
│           │   └── content["application/json"].schema  // 含 $ref → body 模型；否则视为文件上传
│           └── responses["200"]: Response
│               └── content["*/*"].schema  // 返回类型来源
└── components.schemas: Map<String /*模型名*/, Schema>
    └── Schema
        ├── type: Option<String>           // "object"|"array"|"integer"|"string"|"boolean"|"number"|"long"|"file"
        ├── format: Option<String>         // "int64"|"int32"|"double"|"byte"|"binary"...
        ├── items: Option<Box<Schema>>     // array 元素
        ├── $ref: Option<String>           // "#/components/schemas/Xxx"，取末段
        ├── enum: Option<Vec<_>>           // 枚举
        ├── additionalProperties: Option<Box<Schema>>  // map 值类型
        ├── properties: Option<Map<String, Schema>>    // object 字段
        └── description: Option<String>
```

`$ref` 统一取 `split("/")` 的最后一段作为类型名。

---

## 5. ★ 类型映射规则（核心，全部收敛到 `lower/`）

> 这是最容易在迁移中丢失的隐性知识。下面规则来自 `splice.js` / `splice.flutter.js`，
> **必须用语言无关的 IrType 表达**，再由各 codegen 翻译。

### 5.1 IrType 设计建议

```rust
enum IrType {
    Int,                       // 32 位整数
    Long,                      // int64 → 在 TS/Dart 中映射为 String
    Double,
    Bool,
    String,
    Array(Box<IrType>),        // T[]
    Map(Box<IrType>),          // { [key:string]: T }
    Ref(String),               // 引用其它模型
    IPage(Box<IrType>),        // 分页泛型 IPage<T>
    MsgType,                   // 枚举的非 Dto 形态
    File,                      // 文件上传
    Binary,                    // byte/binary → ArrayBuffer
    Any,                       // object
    Void,                      // 无返回
}
```

各语言最终映射表（在 codegen 实现）：

| IrType | TypeScript | JavaScript(JSDoc) | Flutter/Dart |
|--------|-----------|-------------------|--------------|
| Int | `number` | `number` | `int` |
| Long | `string` | `string` | `String` |
| Double | `number` | `number` | `double` |
| Bool | `boolean` | `boolean` | `bool` |
| String | `string` | `string` | `String` |
| Array(T) | `T[]` | `T[]` | `List<T>` |
| Map(T) | `{[key:string]:T}` | `Object` | `Map<String,T>` |
| Ref(N) | `N` | `N` | `N` |
| IPage(T) | `IPage<T>` | `IPage<T>` | `IPage<T>` |
| MsgType | `MsgType` | `MsgType` | `MsgType` |
| File | `File` | `File` | `XFile` |
| Binary | `ArrayBuffer` | `ArrayBuffer` | (按导出处理) |
| Any | `any` | `*` | `dynamic` |
| Void | (无返回) | (无返回) | `bool`（success） |

### 5.2 字段/参数基础类型映射（对应两份 `integerFc`）

输入：`{ type, format, items, $ref, enum, additionalProperties }`，`is_dot`（参数是否来自 query 引用模型的标志，影响枚举）。

按顺序判断：

1. **additionalProperties 存在**（仅 TS 版的 integerFc 在字段层处理 map）→ `Map(integerFc(additionalProperties))`；
   特例：若 `additionalProperties.properties.andIncrement` 存在 → 值类型强制为 `Int`（原 JS：`number`）。
2. **enum 存在** → `is_dot ? Int : MsgType`。
3. **type ∈ {int, integer}** → `format == "int64" ? Long : Int`。
4. **type == "file"** → `File`。
5. **type == "array" 且有 items**：
   - items.type ∈ {int,integer} → 元素 `format=="int64"?Long:Int`
   - items.$ref → `Array(Ref(末段))`
   - 否则 → `Array(String)`（默认）
   - （Flutter 额外：见 5.6 binary 处理）
6. **type == "array" 且无 items** → `Array(String)`。
7. **type == "long"** → `Long`（String）。
8. **$ref 存在**：
   - 末段 == `"LocalTime"` → `String`
   - 否则 → `Ref(末段)`
9. **type == "object"**（Flutter）→ `Any`(dynamic)。
10. **type == "string" 或 "LocalTime"** → `String`。
11. **type == "boolean"/"Boolean"** → `Bool`。
12. **type == "number"** → `format=="double" ? Double : Int`。
13. **format == "binary"**（Flutter，覆盖前面结果）：`Array(String)→Array(File)`，否则 `File`。

> ⚠️ TS 与 Flutter 的 integerFc 略有差异（TS 处理了 additionalProperties / number 处理较少），迁移时以**合并后的完整规则**为准，差异点用注释标注；codegen 阶段不再做类型判断。

### 5.3 ★ 返回类型解析（对应两份 `spliceApiResultType`）

输入：`responses["200"]`。

1. 无 `content` → `Void`（无返回）。
2. `content["*/*"].schema`：
   - `schema.type == "object"` → `Any`。
   - `schema.type` 存在（array 等）：
     - 取 `schema.items.$ref` 末段，或 `schema.items`，或 `schema` 本身。
     - 若 `schema.format == "byte"` → `Binary`(ArrayBuffer)。
     - `schema.type == "array"` → `Array(Ref(types))`。
   - 否则走 `$ref` 末段 `types`：
3. **`types` 解析规则**（`types` = `$ref` 末段，常带 `R` 前缀，表示后端统一响应包装 `R<T>`）：
   - `types[0] != 'R'` → 直接 `Ref(types)`（非包装类型，原样返回）。
   - 去掉首字母 `R` 后的剩余串 `rest = types[1..]`：
     - `rest == "Long"` 或 `"String"` → **TS**: `String`；**Flutter**: `rest=="Long"→String`。
     - `rest` 以 `"MapString"` 开头（即 `types[1..10]=="MapString"`）：剩余 `s = types[10..]`
       - `s` 以 `"List"` 开头 → `Map(Array(Ref(s[4..])))`（TS: `{[key:string]:X[]}`）
       - `s` 以 `"Set"` 开头 → `Map(Array(Ref(s[3..])))`
       - 否则 → `Map(Ref(s))`
     - `types[1..4]=="Map"`（非 MapString）→ **TS**: `Map(Any)`；Flutter 此分支空（未处理）。
     - `types[1..13]=="MapLocalDate"` → `Map(Ref(types[13..]))`。
     - `rest == "Void"` → `Void`（无返回）。
     - `rest == "Int"` 或 `"Integer"` → `Int`。
     - `types[1..5]=="List"`（即 `rest` 以 `List` 开头）：`t = types[5..]`
       - `t ∈ {Long, String}`（Flutter: `{Long, LocalDate}`）→ `Array(String)`
       - `t` 以 `"MapString"` 开头（TS）→ 同上 MapString 解析后包成 Map（注意原 TS 这里返回的是 `{[key:string]:...}` 而非数组，属历史写法，迁移时保留语义但标注）
       - `t == "MapStringString"`（Flutter）→ `Array(Map(String))`
       - `t == "DztccCarType"`（TS 特例）→ `Array(MsgType)`
       - 否则 → `Array(Ref(t))`
     - `types[1..6]=="IPage"` → `IPage(Ref(types[6..]))`。
     - `rest == "SetString"`（TS）→ `Array(String)`。
     - `rest == "Boolean"` → `Bool`。
     - 兜底 → `Ref(rest)`（去掉 R 前缀）。

> 💡 这些 `R前缀 / MapString / IPage` 规则源自特定后端（Java + 统一响应 `R<T>` + MyBatis-Plus `IPage`）。迁移时**完整保留**，并集中在 `lower/result_type.rs` 一个函数里，配测试用例锁定行为。

### 5.4 ★ 导出接口判断

满足任一即视为「文件导出」，返回类型强制为 `Binary`(ArrayBuffer / 按文件处理)：
- 函数名末尾包含 `"export"`（TS：取末 6 字符小写判断；Flutter：整名小写包含）。
- 或 `summary` 包含「导出」。

### 5.5 ★ 接口参数解析（对应两份 `schemaParamsType`）

遍历 `operation.parameters`：
- `parameter.schema.$ref` 存在 → 记为 `{ name: "dot", type: Ref(末段) }`（表示一个引用模型的 query 参数集合）。
- 否则 → `{ name: parameter.name, type: integerFc({type,format}, is_dot=true) }`。

处理 `operation.requestBody`：
- `content["application/json"].schema.$ref` 存在 → `{ name: "vo", type: Ref(末段) }`（请求体模型）。
  - **TS 特例**：若该模型名 == `"LongList"` → 类型记为 `Array(String)`（`string[]`）。
- 否则（无 $ref）→ 视为文件上传：`{ name: "file", type: File/XFile }`。

约定的特殊 name 语义（IR 里应建模为枚举而非裸字符串）：
- `"dot"` = query 引用模型；`"vo"` = JSON 请求体模型；`"file"` = 文件上传；其它 = 普通 path/query 标量参数。

### 5.6 分页处理
- 返回类型为 `IPage<T>` 时：
  - **TS**：参数 interface 额外 `extends Paging`；函数参数 `params` 类型为生成的 interface 或 `Paging`。
  - **Flutter**：参数列表追加 `pageNum: int`、`pageSize: int`。

---

## 6. 函数名生成规则（两份一致）

```
funcName = `{method}{url}`，其中 url 做如下替换：
  "/" → "_"
  "-" → "_"
  "${" → ""   （先处理）
  "}"  → ""
  "{"  → ""
```
例：`GET /user/{id}/info` → `GET_user_id_info`。

- TS 的接口名（PascalCase）：`funcName.split("_").map(titleCase).join("")`，`titleCase` = 首字母大写 + 其余小写。
- Flutter 的方法名（camelCase）：`getCamelCase(funcName)`，按 `_` 分割，首段小写其余首字母大写。

URL 模板还原：
- TS：`url.replace("{","${")` → 模板字符串，最终 `${` → `${params?.`（即 `${params?.id}`）。
- Flutter：`url.replace("{","$").replace("}","")` → `$id` 形式。

---

## 7. 各语言后端生成要点

### 7.1 TypeScript（对应 splice.js）
- 数据模型 → `export interface Name { field?: type; ... }`，字段带 `/** description */`；`description` 含「枚举」→ 字段类型强制 `number`。
- 接口 → 可选 `export interface {PascalName}` 参数对象（含 `extends`/`Paging`），再 `export const funcName = async (...) => { ... }`。
- 请求：`server.{METHOD}<ResultType>(\`url\`, {data/params/responseType})`。
- 文件上传：构造 `FormData`，`config` 中加 `data:formdata`。
- 导出接口：`responseType:'arraybuffer'`，返回 `res as ArrayBuffer` 或 `null`。
- 普通返回：`return res?.result`（数组类型补 `??[]`）。
- 内置类型 `TsxOtherType`：`Paging`、`IPage<T>`、`MsgType`（见 `const.js`）。

### 7.2 Flutter/Dart（对应 splice.flutter.js）
- 数据模型 → `class Name { type? field; ... toJson() {...} Name({this.x}); Name.fromJson(json) {...} }`。
  - 字段 `description` 含「枚举」→ 类型 `int`。
  - `fromJson`/`toJson` 对 List<对象> / 对象 / 基础类型分别处理（见 splice.flutter.js 5 处分支）。
  - `IPage` 模型的 `records` 字段特殊：类型取 `keyname[5..]`。
- 接口 → `Future<Result?> methodName({required ...}) async { ... DioUtil.instance.request<T>(...) ... }`。
  - `fromJson` 回调按返回类型生成（List / IPage / 对象 / 基础类型）。
  - 文件上传：`MultipartFile.fromFile`，构造 `FormData.fromMap`，支持 `XFile` 与 `List<XFile>`。
  - 返回：`res.result`（数组补 `??[]`）或 `res.success`。
- 内置类型 `FlutterOtherType`：`IPage<T>`（含 fromJson）、`MsgType`（见 `const.js`）。

### 7.3 JavaScript（新增，无 JS 原版）
- 以 TypeScript 生成器为基础，去掉所有类型标注（无 `interface`、无 `: type`、无 `<T>`）。
- 类型信息保留在 JSDoc 注释中（`@param {type} name`、`@return`）。
- 请求与返回逻辑同 TS（`server.METHOD(...)`、`return res?.result`）。

---

## 8. 文件输出（对应 analyze.js saveFile）

- 文件名：取 path key 的第一段（`key.split("/")[1]`）作为模块/文件名。
- 实际原版把所有内容 `saveFile(page, "index", pathUrl)` 写到 `{output}/index/index.{ext}`，且同名追加（`appendFile`）、否则清空目录后写入。
- 文件后缀：`flutter → dart`，否则 `ts`（新增 `js`）。
- 内容结构：`header.join("\n")` + 内置类型块 + 生成的接口/模型代码。

> Rust 实现时建议参数化：可选「单文件 index」或「按模块分文件」。先对齐原版（单 index 文件）保证行为一致，再考虑增强。

---

## 9. components.schemas 遍历过滤规则（analyze.js）

- 跳过名字「第 2 个字符是大写字母」的 schema（`key[1]` 的 charCode 在 65~90 之间）——原版用于跳过形如 `IXxx`/`RXxx` 等包装泛型实例。**注意此规则较 hacky，迁移时保留并加测试**。
- 跳过 `"LocalTime"`。
- paths 遍历时跳过 `config.filter` 中列出的 key。
- `deprecated` 接口：`config.deprecated` 为假且 operation.deprecated 为真 → 跳过。

---

## 10. Rust 项目结构（目标）

> 实际 Cargo 包名为 `swagger_api`，二进制名为 `swagger`（`[[bin]]` 指定），npm 主包名 `swagger-api-rs`（§14）。

```
swagger_api/            # Cargo 包（rust 源码）；npm 主包名 swagger-api-rs
├── Cargo.toml          # serde, serde_json, reqwest, clap, anyhow, (insta for tests)
└── src/
    ├── main.rs         # 入口：解析 CLI → 读配置 → fetch → lower → codegen → emit
    ├── cli.rs          # clap，--file 参数
    ├── config.rs       # Config 结构 + 校验（§3）
    ├── fetcher/        # HTTP 拉取（§2）
    │   └── mod.rs
    ├── openapi/        # 原始 spec serde 模型（§4）
    │   └── mod.rs
    ├── ir/             # ★ IR/AST（§5.1）
    │   ├── mod.rs
    │   ├── types.rs    # IrType
    │   ├── model.rs    # IrModel / IrField
    │   └── api.rs      # IrEndpoint / IrParam
    ├── lower/          # ★ spec → IR，所有类型映射（§5）
    │   ├── mod.rs
    │   ├── result_type.rs   # 返回类型解析（§5.3）
    │   ├── param.rs         # 参数解析（§5.5）
    │   └── field_type.rs    # integerFc 合并（§5.2）
    ├── codegen/        # ★ IR → 代码
    │   ├── mod.rs      # trait CodeGenerator
    │   ├── typescript.rs
    │   ├── javascript.rs
    │   └── flutter.rs
    └── emit.rs         # 写文件（§8）
```

### CodeGenerator trait 草案
```rust
pub trait CodeGenerator {
    fn map_type(&self, ty: &IrType) -> String;
    fn gen_model(&self, model: &IrModel) -> String;
    fn gen_endpoint(&self, ep: &IrEndpoint) -> String;
    fn builtin_types(&self) -> &'static str;  // IPage/MsgType/Paging
    fn file_ext(&self) -> &'static str;       // "ts"/"js"/"dart"
}
```

---

## 11. 迁移执行顺序（与 TODO 列表对应）

1. 项目骨架 + CLI
2. config.rs
3. openapi/ 模型
4. fetcher/
5. ir/（★ 先定义好 IR，后续都依赖它）
6. lower/（★ 把 §5 全部规则实现 + 单元测试锁定）
7. codegen/mod.rs trait
8. typescript.rs（先对齐 splice.js）
9. flutter.rs（对齐 splice.flutter.js）
10. javascript.rs（新增）
11. emit.rs
12. 端到端 + snapshot 测试（insta），用现有 `swagger.json` / `swagger.flutter.json` 验证

---

## 12. 验证与防回归

- **黄金样本**：保存几份真实 `api-docs` JSON 作为 fixtures。
- **快照测试**：对每个 codegen 用 `insta` 锁定输出；TS/Flutter 的输出应能与现有 JS 工具产物逐项对照（语义一致，格式可不同）。
- **类型映射单测**：§5.2 / §5.3 每条规则至少一个用例（尤其 `R<T>`、`MapString*`、`IPage`、`List*`、导出、int64）。
- 边界用例：文件上传（单/多）、分页、无返回（Void）、枚举、Dto 后缀、filter 过滤、deprecated。

---

## 13. 已知历史包袱 / 待澄清点（迁移时注意）

1. TS 与 Flutter 的 `integerFc` / `spliceApiResultType` 存在细微差异（见 §5.2/§5.3 标注），合并到 lower 时需保留差异或统一并记录决策。
2. §9 的「第 2 字符大写跳过」规则很 hacky，依赖具体后端命名，迁移后建议加配置开关。
3. 原版输出统一写到 `index/index.{ext}` 单文件，与「按模块分文件」的直觉不同，先对齐原版。
4. TS 版 `List + MapString` 分支历史写法返回 Map 而非数组，语义存疑，保留并标注。
5. `DztccCarType → MsgType[]`、`LongList → string[]` 等是业务特例，建议抽到配置/常量表，而非硬编码。
6. JavaScript 为全新目标，无参照产物，需人工确认输出风格。
7. **path 参数不应重复作为 query 传递**（原版 bug，Rust 版已借 IR 的 `in_path` 标记修正）：
   当唯一标量参数是 path 参数（已在 URL 模板 `${params?.id}` 插值），axios 第二参数
   `{params}` 多余，应省略；仅当存在「非 path 的 query 标量 / dot / 分页」时才传 `params`。
   TS 生成器已实现（`axios_config` 用 `!p.in_path` 判断），**Flutter / JS 生成器需同样处理**。
8. **vo（请求体）不应进 params interface 的 extends**（原版 bug，已修正）：
   原版把 `vo` 和 `dot` 都放进 `extends`，但 `vo` 由 `data` 单独接收，不属于 params 对象，
   只有 `dot`（query 引用模型）才该 extends。TS 用统一的 `params_kind()` 判断
   （None / DotDirect / PagingDirect / Interface），**Flutter / JS 生成器需沿用「vo 不进 extends」**。
9. **Java 基础类型名不应泄漏到产物**（原版 bug，已在 lower 修正）：
   `R<T>` 解包时，拼接在类型名里的 `Integer`/`Boolean`/`Long`/`Double`/`BigDecimal`/`Object`/
   日期类（如 `RMapStringInteger` → `{[key:string]:Integer}`）会把 Java 类型名当成模型 Ref 泄漏。
   修正：`lower/result_type.rs` 的 `ref_or_primitive()` 统一映射基础类型名 → IrType，否则才 Ref，
   应用于 MapString 值 / List 元素 / IPage 内层 / MapLocalDate / 非 R 前缀 / 兜底分支。
   因在 lower 层修正，**三种语言后端共同受益**。

---

## 14. npm 发布方案（预编译二进制 + optionalDependencies）

> 采用业界标准方案（Biome / esbuild / SWC / Turbo 同款）：CI 交叉编译多平台原生二进制，
> 每个平台一个 npm 子包，主包用极薄 JS 启动器按当前平台选对应二进制执行。
> **安装快、无 postinstall、无运行时网络依赖、内网/CI 友好。**

### 14.1 包结构

```
swagger-api-rs/                       # 主包（用户 npm i -g 的就是它）
├── package.json                      # bin: { "swagger": "bin/swagger.js" }
│                                     # optionalDependencies: 各平台子包
├── bin/swagger.js                    # 薄 JS 启动器（见 14.3）
└── README.md

@swagger-api/cli-darwin-arm64/        # 平台子包（每平台一个，仅含二进制）
├── package.json                      # os: ["darwin"], cpu: ["arm64"]
└── swagger                           # Rust 编译出的二进制
@swagger-api/cli-darwin-x64/
@swagger-api/cli-linux-x64/
@swagger-api/cli-win32-x64/           # 二进制名 swagger.exe
@swagger-api/cli-linux-arm64/         # （按需）
```

要点：
- 子包用 `os` / `cpu` 字段限定平台，npm 安装时**只会装当前平台那一个** optionalDependency，其余自动跳过。
- 子包加 `"bin"` 或直接暴露二进制路径，主包用 `require.resolve` 定位。
- 主包**不含任何二进制**，体积极小。

### 14.2 主包 package.json 关键字段

```jsonc
{
  "name": "swagger-api-rs",
  "version": "2.0.0",
  "bin": { "swagger": "bin/swagger.js" },
  "optionalDependencies": {
    "@swagger-api/cli-darwin-arm64": "2.0.0",
    "@swagger-api/cli-darwin-x64":   "2.0.0",
    "@swagger-api/cli-linux-x64":    "2.0.0",
    "@swagger-api/cli-linux-arm64":  "2.0.0",
    "@swagger-api/cli-win32-x64":    "2.0.0"
  },
  "files": ["bin"]
}
```
> 子包版本号必须与主包**严格一致**（发布脚本统一 bump），否则启动器找不到匹配版本。

### 14.3 启动器 bin/swagger.js（核心逻辑）

```js
#!/usr/bin/env node
const { execFileSync } = require("node:child_process");

function resolveBinary() {
  const { platform, arch } = process;
  const ext = platform === "win32" ? ".exe" : "";
  const pkg = `@swagger-api/cli-${platform}-${arch}`;
  try {
    // 子包内二进制名固定为 swagger / swagger.exe
    return require.resolve(`${pkg}/swagger${ext}`);
  } catch {
    throw new Error(
      `不支持的平台 ${platform}-${arch}，或子包未安装（请检查 optionalDependencies 是否被 --no-optional / --ignore-scripts 跳过）`
    );
  }
}

try {
  execFileSync(resolveBinary(), process.argv.slice(2), { stdio: "inherit" });
} catch (e) {
  process.exit(e.status ?? 1);
}
```
> `stdio: "inherit"` 保证 Rust 端 stdout/stderr 直通终端；`e.status` 透传退出码。

### 14.4 Cargo 交叉编译目标（target → npm 平台映射）

| Rust target | npm 子包 | os / cpu |
|-------------|----------|----------|
| `aarch64-apple-darwin` | cli-darwin-arm64 | darwin / arm64 |
| `x86_64-apple-darwin` | cli-darwin-x64 | darwin / x64 |
| `x86_64-unknown-linux-gnu` | cli-linux-x64 | linux / x64 |
| `aarch64-unknown-linux-gnu` | cli-linux-arm64 | linux / arm64 |
| `x86_64-pc-windows-msvc` | cli-win32-x64 | win32 / x64 |

> Linux 若要兼容老旧 glibc，可考虑 `*-musl` 静态链接（musl）目标，避免 glibc 版本问题。

### 14.5 CI 发布流程（GitHub Actions matrix）

1. 打 tag（如 `v2.0.0`）触发 workflow。
2. **matrix 交叉编译**：每个 target 一个 job，`cargo build --release --target <t>`，产出二进制 upload 为 artifact。
3. **打子包**：脚本把各平台二进制拷进对应子包目录，写好子包 `package.json`（含 os/cpu/version），`npm publish --access public`。
4. **发主包**：所有子包发完后再发主包（保证用户安装时 optionalDeps 已存在）。
5. 版本统一：用脚本（或 changesets / release-it）一次性 bump 主包与所有子包到同一版本。

### 14.6 与现有发布的差异（对比当前 release-it 流程）
- 现有 `.release-it.json` + `npm pack` 单包模式 → 改为「多包 + matrix CI」。
- 现有 `package.json` 的 `bin: { swagger: ./src/index.js }` → 改为 `bin/swagger.js` 启动器。
- 包名建议升大版本（如 `swagger-api-ts` → 维持名或新名 `swagger-api-rs`），README 更新安装方式。

### 14.7 本地验证发布物
- `npm pack` 主包与某个子包，在干净目录 `npm i ./*.tgz` 验证启动器能定位并执行二进制。
- 测试 `--ignore-scripts` 场景（本方案应仍可用，因为不依赖 postinstall）。
- 测试缺失平台时的报错信息是否清晰（14.3 的错误提示）。

---

## 15. 第二阶段：增量变更对比（diff）

每次生成时与上次结果做**语义级 diff** 并打印变更，帮助使用者了解 API 改了什么。

### 15.1 架构（复用第一阶段 IR）

```
lower(新IR) ─┬─> codegen + emit（全量重写）
             └─> cache::load(旧IR) ─> diff(旧,新) ─> report 打印
                                                  └─> cache::save(新IR)
```

关键决策：**在 IR 层做 diff**（非反解生成代码、非文本 diff）。IR 已是语言无关 AST，
一份 diff 逻辑对 TS/JS/Dart 三种产物都生效，无需为目标语言写解析器。

### 15.2 模块

| 模块 | 职责 |
|------|------|
| `ir`(IrCache/CACHE_VERSION) | IR 序列化外壳，版本化 |
| `cache` | 读/写 `{output}/.swagger-ir.json`；缺失/损坏/版本不符 → 降级为首次 |
| `diff` | 对比两 IrModule → IrDiff（接口/模型/字段 增删改 + 重命名启发式） |
| `report` | 渲染变更到控制台（ANSI 颜色，非 tty 自动关闭） |

### 15.3 要点

- **缓存存规范化后的 IR**（§21 名称去重之后），保证 diff 稳定。
- **重命名启发式**：接口按 `方法+参数+返回类型+导出` 一致配对；模型按字段集 Jaccard 相似度（阈值 0.7）配对；全局贪心、确定性、一对一。
- **确定性**：lowering / 规范化 / 重命名规则均确定性，避免假变更（diff 抖动）。有 `lowering_is_deterministic` 测试守护。
- **CLI**：`--no-diff`（静默，仍写文件+缓存）、`--diff-only`（仅预览，不落盘），互斥。
