# AI-Light 项目文档

本目录用于集中维护项目工程化文档。文档内容应与仓库中的实际配置同步，尚未实现的能力必须标记为规划项。

## 核心规范

- [Hook API 使用指南](./specs/hook-api.md) / OpenAPI 3.1（运行时由 Rust 代码生成）
- [主题格式指南](./specs/theme-format.md) / [Theme JSON Schema](./specs/theme.schema.json)（由 Rust DTO 生成）
- [IPC 契约](./specs/ipc-contract.md)
- [技术架构](./specs/architecture.md)
- [UI 交互说明](./specs/ui-interactions.md) / [UI 组件契约](./specs/ui-interaction-spec.md)

## CI/CD

- [建设进展](./ci-cd/status.md)：已完成、待验证和待建设事项
- [持续集成](./ci-cd/continuous-integration.md)：触发条件、检查内容和运行环境
- [发布操作手册](./ci-cd/release-runbook.md)：版本发布、验收和回滚步骤
- [故障排查](./ci-cd/troubleshooting.md)：常见失败原因和处理方式

## 维护约定

涉及工作流、构建环境、版本策略、签名或发布方式的变更，应同步更新本目录。进展状态统一使用：

- `已完成`：代码已落库且可本地验证
- `待线上验证`：配置已完成，但尚未经过 GitHub Actions 实际运行
- `未实施`：仅列入后续路线，不代表当前具备该能力
