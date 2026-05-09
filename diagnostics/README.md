# 诊断目录

这里保存 `python scripts\export_diagnostics.py` 生成的诊断包。

诊断包目录格式：

```text
bundle_YYYYMMDD_HHMMSS
```

这些目录是生成物，在调查结束后可以删除。

不要把以下内容放进诊断包：

- API Key
- token
- cookie
- 完整用户 `.litematic` 文件
- 大型 cache 二进制文件

诊断包字段说明见：

```text
docs/DIAGNOSTICS.md
```

