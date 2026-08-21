# AI-Light 主题格式指南

| 项目 | 内容 |
|---|---|
| 格式版本 | V1 |
| 文件扩展名 | `.ailight-theme.json` |
| 机器契约 | [JSON Schema Draft 2020-12](./theme.schema.json) |
| 示例主题 | [内置主题目录](./themes/) |

主题描述“业务状态应该呈现什么灯光与声音”。设备只执行编译后的场景，不理解 `WORKING`、`ERROR` 等业务语义，因此更换主题不需要修改设备协议或固件。

## 1. 概念模型

```text
theme    主题名称与格式版本
scenes   可复用的灯光/声音场景库
states   业务状态到场景名称的映射
```

处理链路：

```text
Hook 状态事件 → 状态仲裁 → states 查找 → scenes 编译 → 设备场景
```

`scenes` 与 `states` 分离可以让多个状态复用一个场景，也可以只修改场景而不改变业务映射。

## 2. 最小主题

```json
{
  "theme": {
    "name": "minimal-blue",
    "version": 1
  },
  "scenes": {
    "off": {
      "leds": [null, null, null]
    },
    "blue": {
      "leds": [
        {
          "curve": "CONSTANT",
          "high": "#168CFF",
          "brightness": 60
        },
        null,
        null
      ]
    }
  },
  "states": {
    "IDLE": {
      "scene": "off"
    },
    "WORKING": {
      "scene": "blue"
    }
  }
}
```

## 3. 顶层结构

| 字段 | 类型 | 说明 |
|---|---|---|
| `theme` | object | 元信息；包含 `name` 与固定值 `version: 1` |
| `scenes` | object | 至少一个命名场景 |
| `states` | object | 状态到场景的映射，可以为空 |

主题名、场景名和状态名统一使用 `[A-Za-z0-9_-]+`，长度为 1 至 64 字符。顶层和各层对象的未知字段都会导致整个主题被拒绝。

## 4. 场景结构

每个场景包含恰好三条灯轨 `leds`，按顶、中、底顺序排列；某一项为 `null` 表示该灯输出黑色。`buzzer` 可省略或设为 `null` 表示静音。

```json
{
  "leds": [null, null, null],
  "buzzer": null
}
```

### 4.1 灯轨

| 字段 | 类型 | 说明 |
|---|---|---|
| `curve` | enum | `CONSTANT / SQUARE / TRIANGLE / SAW_UP / SAW_DOWN` |
| `high` | `#RRGGBB` | 波形高点颜色 |
| `low` | `#RRGGBB` | 可选，波形低点颜色 |
| `brightness` | integer 0–100 | 整条灯轨亮度 |
| `period_ms` | integer | 非 `CONSTANT` 必填且大于 0 |
| `phase_deg` | integer 0–360 | 可选，三灯错峰运动的相位 |
| `repeat` | integer 0–65535 | 可选；0 或省略表示持续 |
| `end_level` | enum | 可选；有限重复结束后的 `OFF / LOW / HIGH` |

曲线条件：

| 曲线 | 约束 |
|---|---|
| `CONSTANT` | `period_ms / phase_deg / duty_percent / repeat` 只能省略或为 0 |
| `SQUARE` | `period_ms` 必须大于 0；`duty_percent` 必填，范围 1–99 |
| 其他波形 | `period_ms` 必须大于 0；`duty_percent` 只能省略或为 0 |

主题静态校验负责结构和基础范围；设备实际支持的周期范围在连接后根据能力信息检查。

### 4.2 蜂鸣轨

```json
{
  "start_delay_ms": 0,
  "repeat": 3,
  "segments": [
    {
      "frequency_hz": 2000,
      "duration_ms": 150,
      "volume": 70
    },
    {
      "frequency_hz": 0,
      "duration_ms": 150,
      "volume": 0
    }
  ]
}
```

| 字段 | 约束 |
|---|---|
| `start_delay_ms` | 可选，0–65535 |
| `repeat` | 可选，0–65535；0 或省略表示持续 |
| `segments` | 必填，1–16 段 |
| `frequency_hz` | 0–65535；0 表示静音间隔 |
| `duration_ms` | 1–65535 |
| `volume` | 0–100 |

频率的实际可用范围由设备能力决定，Schema 只表达协议字段可承载的静态范围。

## 5. 状态映射

```json
{
  "WORKING": {
    "scene": "breath-blue",
    "transition_ms": 300
  },
  "SUCCESS": {
    "scene": "success-green",
    "hold_ms": 5000
  }
}
```

| 字段 | 约束 | 语义 |
|---|---|---|
| `scene` | 必填，合法名称 | 必须引用当前文件 `scenes` 中存在的场景 |
| `transition_ms` | 0–2500 | 进入状态时的过渡时长 |
| `hold_ms` | 非负整数 | 终态驻留后回落 `IDLE`；0 表示不自动回落 |

标准状态为 `IDLE / WORKING / WAITING / SUCCESS / ERROR`，也可以增加自定义状态。未映射的状态按 `IDLE` 呈现并写入日志；若 `IDLE` 也未映射，则输出熄灭场景。

## 6. 两层校验

主题导入执行两层校验：

1. **JSON Schema 结构校验**：类型、必填字段、枚举、范围、名称格式、条件字段和未知字段。
2. **运行时语义校验**：`states.*.scene` 跨对象引用、设备能力边界及编译约束。

标准 JSON Schema 无法可靠声明“对象值必须引用同一文档另一个动态键”，所以场景引用由 Rust 校验器负责。任一层失败都会整体拒绝主题，不做部分加载。

### 编辑器配置

VS Code 可按扩展名关联 Schema：

```json
{
  "json.schemas": [
    {
      "fileMatch": ["*.ailight-theme.json"],
      "url": "./docs/specs/theme.schema.json"
    }
  ]
}
```

也可使用支持 JSON Schema Draft 2020-12 的工具直接加载 `theme.schema.json`。

## 7. 创作建议

- 先定义语义明确、可复用的场景名，再映射状态。
- `CONSTANT` 适合静态提示，`TRIANGLE` 适合呼吸，`SQUARE` 适合警报，`SAW_UP/SAW_DOWN` 适合方向性运动。
- 使用三条轨道不同的 `phase_deg` 形成从上到下、从下到上或交错运动。
- 提示音尽量短；持续蜂鸣会干扰工作环境。
- 一个场景被多个状态引用时，修改它会同时影响所有引用状态。
- 内置主题只能作为创作起点另存为用户主题，不能被同名用户文件覆盖。

## 8. 版本与兼容性

V1 文件必须声明 `theme.version: 1`。未来格式若增加破坏性字段，将递增该版本并发布新的 Schema。主题文件不携带设备协议版本；客户端负责把稳定的主题表达编译到当前设备协议。

维护约定：

- Rust Theme DTO、字段注释与 `JsonSchema` 实现是结构契约的唯一事实源；[theme.schema.json](./theme.schema.json) 是其生成产物。
- 本文解释概念、工作流与字段语义，不取代机器校验。
- 修改 DTO 后运行 `cd crates/ailight-core && cargo run --example generate_theme_schema` 更新生成产物。
- 测试会检查生成产物与 DTO 完全同步，并让六个内置主题同时经过 JSON Schema 与运行时语义校验。
- 修改字段时必须同步 Schema、Rust 数据结构、内置主题和 UI 主题编辑器契约。

相关文档：

- [内置主题说明](./themes/README.md)
- [Hook API](./hook-api.md)
- [IPC 契约](./ipc-contract.md)
- [ADR-0002](../decisions/ADR-0002-主题格式设计决策.md)
