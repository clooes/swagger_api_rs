#!/usr/bin/env node
// 组装 npm 发布物（见 MIGRATION.md §14）。
//
// 用法：
//   node scripts/build-npm.mjs <version> [--publish]
//
// 输入：编译好的二进制位于 artifacts/<target>/swagger[.exe]
// 产出：dist-npm/ 下生成主包与各平台子包（已注入 version）
// --publish：依次 npm publish（先所有子包，后主包），需 NODE_AUTH_TOKEN

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

// 单一平台清单：Rust target → npm 平台
const PLATFORMS = [
  { target: "aarch64-apple-darwin", os: "darwin", cpu: "arm64" },
  { target: "x86_64-apple-darwin", os: "darwin", cpu: "x64" },
  { target: "x86_64-unknown-linux-gnu", os: "linux", cpu: "x64" },
  { target: "aarch64-unknown-linux-gnu", os: "linux", cpu: "arm64" },
  { target: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64" },
];

const MAIN_PKG = "swagger-api-rs";

const version = process.argv[2];
const doPublish = process.argv.includes("--publish");
if (!version || version.startsWith("--")) {
  console.error("用法: node scripts/build-npm.mjs <version> [--publish]");
  process.exit(1);
}

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const artifactsDir = path.join(root, "artifacts");
const distDir = path.join(root, "dist-npm");
const mainSrc = path.join(root, "npm", MAIN_PKG);

function subPkgName(p) {
  return `swagger-api-rs-${p.os}-${p.cpu}`;
}

function rmrf(p) {
  fs.rmSync(p, { recursive: true, force: true });
}

function writeJson(file, obj) {
  fs.writeFileSync(file, JSON.stringify(obj, null, 2) + "\n");
}

// 1. 准备 staging
rmrf(distDir);
fs.mkdirSync(distDir, { recursive: true });

// 2. 生成各平台子包
const subDirs = [];
for (const p of PLATFORMS) {
  const name = subPkgName(p);
  const ext = p.os === "win32" ? ".exe" : "";
  const binSrc = path.join(artifactsDir, p.target, `swagger${ext}`);
  if (!fs.existsSync(binSrc)) {
    console.warn(`⚠️  跳过 ${name}：未找到二进制 ${binSrc}`);
    continue;
  }
  const pkgDir = path.join(distDir, name);
  fs.mkdirSync(path.join(pkgDir, "bin"), { recursive: true });

  const binDst = path.join(pkgDir, "bin", `swagger${ext}`);
  fs.copyFileSync(binSrc, binDst);
  if (p.os !== "win32") fs.chmodSync(binDst, 0o755);

  writeJson(path.join(pkgDir, "package.json"), {
    name,
    version,
    description: `swagger-api-rs 预编译二进制 (${p.os}-${p.cpu})`,
    os: [p.os],
    cpu: [p.cpu],
    license: "MIT",
    files: ["bin"],
  });
  subDirs.push(pkgDir);
  console.log(`✓ 子包 ${name}@${version}`);
}

// 3. 生成主包（注入 version 与 optionalDependencies 版本）
const mainDir = path.join(distDir, MAIN_PKG);
fs.cpSync(mainSrc, mainDir, { recursive: true });
const mainPkg = JSON.parse(fs.readFileSync(path.join(mainDir, "package.json"), "utf8"));
mainPkg.version = version;
mainPkg.optionalDependencies = {};
for (const p of PLATFORMS) {
  mainPkg.optionalDependencies[subPkgName(p)] = version;
}
writeJson(path.join(mainDir, "package.json"), mainPkg);
console.log(`✓ 主包 ${MAIN_PKG}@${version}`);

// 4. 发布（先子包后主包，保证用户安装主包时 optionalDeps 已存在）
if (doPublish) {
  for (const dir of subDirs) {
    console.log(`发布 ${path.basename(dir)} …`);
    execFileSync("npm", ["publish", "--access", "public"], { cwd: dir, stdio: "inherit" });
  }
  console.log(`发布 ${MAIN_PKG} …`);
  execFileSync("npm", ["publish", "--access", "public"], { cwd: mainDir, stdio: "inherit" });
  console.log("✅ 发布完成");
} else {
  console.log(`\n已在 ${distDir} 生成发布物（未发布）。加 --publish 实际发布。`);
}
