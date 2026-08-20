# AI-Light 主题格式规范（Theme Format）

| 项目 | 内容 |
|---|---|
| 文档版本 | V1.0（正式版） |
| 文档状态 | 生效（ADR-0002 确定） |
| 文件扩展名 | `.ailight-theme.json`（单文件，可分享/导入） |
| 生效日期 | 2026-08-19 |

> 主题 = `状态 → SCENE` 映射表。客户端加载主题后，把接入层收到的标准状态编译为 V0.4 SCENE 下发给硬件。

---

## 1. 总体结构

```jsonc
{
  "theme": {
    "name": "default",          // 主题名，必填
    "version": 1                // 主题格式版本，必填，当前 = 1
  },
  "scenes": { … },              // 命名 SCENE 库（复用单元）
  "states": { … }               // 状态 → SCENE 名映射
}
```

- 顶层只允许三个键：`theme` / `scenes` / `states`
- 未知键 → 校验失败（整体拒绝，回退默认主题）

## 2. SCENE 库（scenes）

键 = SCENE 名（`[a-zA-Z0-9_-]+`，必填），值 = SCENE 对象：

```jsonc
{
  "breath-blue": {
    "leds": [ …3 条轨道，可含 null… ],
    "buzzer": null              // 可选；null = 静音
  }
}
```

### 2.1 LedTrack（灯轨道，数组长度 3，对应 顶/中/底）

每条轨道可为 `null`（该灯不参与本 SCENE，输出黑色）：

| 字段 | 类型 | 必填 | 说明 | 协议映射（V0.4） |
|---|---|---|---|---|
| `curve` | string | ✅ | `CONSTANT` / `SQUARE` / `TRIANGLE` / `SAW_UP` / `SAW_DOWN` | curve 枚举 |
| `low` | hex color | 条件 | 波形低点颜色 `#RRGGBB`（CONSTANT 时忽略，须省略或 `#000000`） | low_rgb |
| `high` | hex color | ✅ | 波形高点颜色 | high_rgb |
| `brightness` | int 0~100 | ✅ | 整轨亮度；0 = 全黑，但轨道时间仍正常推进 | brightness |
| `period_ms` | int | 条件 | 完整周期；0 或省略 = CONSTANT 静态轨 | period_ms |
| `phase_deg` | int 0~360 | 条件 | 相位（角度制，编译时换算 `phase = phase_deg × 65536 / 360`） | phase |
| `duty_percent` | int 1~99 | 条件 | 仅 SQUARE 有效；非 SQUARE 须省略 | duty_percent |
| `repeat` | int | 条件 | 有限次数；0 或省略 = 持续 | repeat_count |
| `end_level` | string | 条件 | `OFF` / `LOW` / `HIGH`；有限次数时的终态 | end_level |

**约束（与 V0.4 §8.2 校验规则一致，主题层先行校验）**：
- `CONSTANT`：只填 `high` + `brightness`，其余字段省略
- 非 `SQUARE`：不得出现 `duty_percent`
- `period_ms = 0` 仅 CONSTANT 允许；否则必须在设备能力范围 `[min_period_ms, max_period_ms]`（运行时以 GET_CAPABILITIES 为准，主题静态校验只查 0 与非 0）

### 2.2 BuzzerTrack（蜂鸣轨道）

`null` = 本 SCENE 静音。对象结构：

```jsonc
"buzzer": {
  "start_delay_ms": 0,          // 可选，默认 0
  "repeat": 3,                  // 可选，0/省略 = 持续循环
  "segments": [                 // 1~16 条
    { "frequency_hz": 2000, "duration_ms": 150, "volume": 70 },
    { "frequency_hz": 0,    "duration_ms": 150, "volume": 0 }   // 0 Hz = 静音间隔
  ]
}
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `frequency_hz` | int | ✅ | 0 = 静音间隔；否则运行时须在能力范围 `[min_frequency_hz, max_frequency_hz]` |
| `duration_ms` | int | ✅ | 必须 > 0 |
| `volume` | int 0~100 | ✅ | 音量；0 = 静音，`frequency_hz = 0` 时设备忽略该值 |

## 3. 状态映射（states）

键 = 状态名（标准 5 态或自定义，`[a-zA-Z0-9_-]+`），值：

```jsonc
"WORKING": {
  "scene": "breath-blue",       // 必填：引用 scenes 中的 SCENE 名
  "transition_ms": 300,         // 可选：进入本状态的切换过渡时长；0/省略 = 立即
  "hold_ms": 0                  // 可选：终态驻留时长（见下）
}
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `scene` | string | ✅ | 引用 `scenes` 中的 SCENE 名；引用不存在 → 校验失败 |
| `transition_ms` | int | 否 | 状态切换过渡（V0.4 transition_ms）；0/省略 = 立即切换 |
| `hold_ms` | int | 否 | **仅终态语义（SUCCESS/ERROR）有意义**：0 = 驻留到下一事件；N = N 毫秒后自动回落 IDLE；省略 = 0 |

**IDLE 特殊语义**：主题中可写 `IDLE`（指定空闲灯效），不写则内置"熄灭"。
**未映射状态**：客户端收到 `states` 中未出现的状态 → 按 IDLE 显示 + 记日志。

## 4. 校验与容错

1. 加载时**整体校验**（JSON 合法性、字段范围、SCENE 引用完整性）；
2. 任一字段非法 → **整个主题拒绝生效**，回退内置默认主题（随客户端分发，含 5 态基础灯效）；
3. 校验失败与回退原因写入客户端日志；
4. 运行时设备能力（period/frequency 边界）不符 → 按 V0.4 校验规则返回错误，灯保持原状（主题本身仍有效，能力边界以设备为准）。

## 5. 完整示例

```jsonc
{
  "theme": { "name": "default", "version": 1 },
  "scenes": {
    "off": {
      "leds": [null, null, null],
      "buzzer": null
    },
    "breath-blue": {
      "leds": [
        { "curve": "TRIANGLE", "low": "#003366", "high": "#00CCFF", "brightness": 60, "period_ms": 1200, "phase_deg": 0 },
        { "curve": "TRIANGLE", "low": "#003366", "high": "#00CCFF", "brightness": 60, "period_ms": 1200, "phase_deg": 120 },
        { "curve": "TRIANGLE", "low": "#003366", "high": "#00CCFF", "brightness": 60, "period_ms": 1200, "phase_deg": 240 }
      ],
      "buzzer": null
    },
    "breath-amber": {
      "leds": [
        { "curve": "TRIANGLE", "low": "#3D2B00", "high": "#FFB400", "brightness": 50, "period_ms": 1800, "phase_deg": 0 },
        { "curve": "TRIANGLE", "low": "#3D2B00", "high": "#FFB400", "brightness": 50, "period_ms": 1800, "phase_deg": 0 },
        { "curve": "TRIANGLE", "low": "#3D2B00", "high": "#FFB400", "brightness": 50, "period_ms": 1800, "phase_deg": 0 }
      ],
      "buzzer": null
    },
    "glow-green": {
      "leds": [
        { "curve": "CONSTANT", "high": "#00E676", "brightness": 70 },
        { "curve": "CONSTANT", "high": "#00E676", "brightness": 70 },
        { "curve": "CONSTANT", "high": "#00E676", "brightness": 70 }
      ],
      "buzzer": null
    },
    "alert-red": {
      "leds": [
        { "curve": "SQUARE", "high": "#FF0000", "brightness": 80, "period_ms": 400, "duty_percent": 50, "repeat": 5, "end_level": "OFF" },
        null,
        null
      ],
      "buzzer": {
        "start_delay_ms": 0, "repeat": 3,
        "segments": [
          { "frequency_hz": 2000, "duration_ms": 150, "volume": 70 },
          { "frequency_hz": 0,    "duration_ms": 150, "volume": 0 }
        ]
      }
    }
  },
  "states": {
    "IDLE":     { "scene": "off" },
    "WORKING":  { "scene": "breath-blue", "transition_ms": 300 },
    "WAITING":  { "scene": "breath-amber" },
    "SUCCESS":  { "scene": "glow-green", "hold_ms": 5000 },
    "ERROR":    { "scene": "alert-red", "hold_ms": 0 },
    "REVIEW":   { "scene": "breath-purple" }
  }
}
```

## 6. 编译链路（客户端 L2 → L3）

```text
标准状态（如 WORKING）
  → states[WORKING].scene 取 SCENE 名
  → scenes[SCENE 名] JSON 编译为 V0.4 OutputScene（字节级）
  → SET_SCENE（apply_mode 幂等，见协议 §8.4）
```

- 颜色 hex → RGB 字节；`phase_deg` → 归一化 phase；曲线名 → 枚举值
- 编译结果与当前有效 SCENE 去重（`APPLY_IF_CHANGED`）；业务离开后重入或试听 → `RESTART_SCENE`

## 7. 与协议 V0.4 的关系

- 主题格式是**协议无关的表达层**：所有字段最终编译为 V0.4 SCENE
- 主题文件**不包含**协议版本/设备能力信息——能力适配在编译期（L3）完成
- 未来协议升级（如 V0.5 增加新曲线）：主题格式可加字段并递增 `version`，旧主题仍可加载（缺省字段 = 默认值）

## 8. 主题创作器表达约定

主题文件保留精确的协议映射字段，但默认 UI 使用用户可理解的效果语言生成这些字段：

| 用户选择 | 主题字段 |
|---|---|
| 常亮 / 呼吸 / 闪烁 / 渐亮 / 渐弱 | `CONSTANT / TRIANGLE / SQUARE / SAW_UP / SAW_DOWN` |
| 舒缓 / 适中 / 活跃 | 预设 `period_ms` |
| 一起 / 从上往下 / 从下往上 / 交错 | 三灯预设 `phase_deg` 组合 |
| 光线强度 | 三灯 `brightness` |
| 无声 / 轻提示 / 确认音 / 警报音 | 预设 `buzzer.segments` |

- 默认界面不得要求用户理解“相位”“占空比”等协议术语。
- 精确字段只在轨道工作台或 JSON 视图中渐进披露。
- 自定义状态与标准状态使用同一 `states → scenes` 映射机制。
- 一个 SCENE 被多个状态引用时，编辑器必须提示影响范围。
- 内置主题只能作为创作起点另存为新主题；用户主题不得与内置主题同名。
