# 打包说明

## 依赖

1. 安装 Node.js 18+
2. 安装 Rust（[rustup](https://rustup.rs/)）
3. macOS 需要 Xcode Command Line Tools；Windows 需要 WebView2（系统通常已带）

## 开发

```bash
npm install
npm run tauri dev
```

## 发布构建

```bash
npm run tauri build
```

产物目录：

- macOS: `src-tauri/target/release/bundle/dmg/`、`macos/`
- Windows: `src-tauri/target/release/bundle/msi/`、`nsis/`

## CLI 独立二进制

```bash
cargo build --manifest-path src-tauri/Cargo.toml --release --bin devicesim-cli
```

可执行文件：`src-tauri/target/release/devicesim-cli`
