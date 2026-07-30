# vtrans-security

凭据安全模块。通过 Windows Credential Manager 安全存储和读取 API Key。

## 职责

- CredentialManager：store/load/delete API Key
- mask_key：日志安全的 Key 展示

## 依赖

vtrans-core

## 构建

```powershell
cargo build -p vtrans-security
cargo test -p vtrans-security
```

## 详细规格

参见 docs/modules/03-security.md
