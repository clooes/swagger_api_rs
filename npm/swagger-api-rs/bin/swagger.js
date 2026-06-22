#!/usr/bin/env node
"use strict";
// 薄启动器：按当前平台定位对应子包里的原生二进制并执行（见 MIGRATION.md §14.3）。
const { execFileSync } = require("node:child_process");

function binaryPath() {
  const { platform, arch } = process;
  const ext = platform === "win32" ? ".exe" : "";
  // 包名段：win32 → windows（避开 npm 对 "win32" 的反垃圾拦截）
  const osSeg = platform === "win32" ? "windows" : platform;
  const pkg = `swagger-api-rs-${osSeg}-${arch}`;
  try {
    // 子包内二进制固定位于 bin/swagger[.exe]
    return require.resolve(`${pkg}/bin/swagger${ext}`);
  } catch {
    throw new Error(
      `不支持的平台 ${platform}-${arch}：缺少可选依赖 ${pkg}。\n` +
        `请确认未使用 --no-optional / --ignore-scripts，且该平台已发布对应子包。`
    );
  }
}

try {
  execFileSync(binaryPath(), process.argv.slice(2), { stdio: "inherit" });
} catch (e) {
  // 透传子进程退出码；否则按一般错误处理
  if (e && typeof e.status === "number") process.exit(e.status);
  console.error(e && e.message ? e.message : e);
  process.exit(1);
}
