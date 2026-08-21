# 内置主题集（Built-in Themes）

| 项目 | 内容 |
|---|---|
| 文档版本 | V1.0 |
| 规范依据 | `docs/specs/theme-format.md`（V1.0） |
| 文件格式 | `.ailight-theme.json`（单文件，可分享/导入） |
| 数量 | 6 套（随客户端编译进二进制分发） |

> 所有主题遵守"主题集宪法"：**视觉强度严格对齐仲裁优先级 `ERROR > SUCCESS > WORKING > WAITING > IDLE`**——风格可换，语义强度层级不可破。IDLE 均不显式定义（内置熄灭语义）。

---

## 主题清单

| 文件 | 主题名 | 风格 | 服务人群 | 主打硬件特性 |
|---|---|---|---|---|
| `default.ailight-theme.json` | Default 经典 | 均衡克制 | 大多数人 | TRIANGLE 呼吸 / CONSTANT 常亮 / SQUARE 闪烁 / 蜂鸣一声 |
| `minimal.ailight-theme.json` | Minimal 极简 | 单色白灰、几乎无动画、全静音 | 极简主义者、办公环境 | CONSTANT 为主 + 唯一慢呼吸（WAITING） |
| `neon.ailight-theme.json` | Neon 霓虹 | 高饱和荧光、快节奏 | 电竞、科技感爱好者 | **SAW_UP 扫光跑马（相位 120°）+ 全套蜂鸣（SUCCESS 提示音 + ERROR 三连）** |
| `nature.ailight-theme.json` | Nature 自然 | 低饱和大地色、超慢呼吸、长过渡 | 安静偏好者 | 最长过渡（800~1000ms）+ 最低蜂鸣音量 |
| `aurora.ailight-theme.json` | Aurora 极光 | 三灯异色慢速流动 | 多彩偏好者 | **3 颗独立 RGB 各自发光**（WORKING/SUCCESS 三色，相位 120° 流动） |
| `focus.ailight-theme.json` | Focus 专注 | 只亮中间单灯、低亮度 | 深度工作、低干扰 | **单灯表达**（顶/底 null）+ 最低亮度档 |

## 硬件特性覆盖矩阵

| 特性 | Default | Minimal | Neon | Nature | Aurora | Focus |
|---|---|---|---|---|---|---|
| CONSTANT 常亮 | ✓ SUCCESS | ✓ 三态 | ✓ SUCCESS | ✓ SUCCESS | ✓ SUCCESS | ✓ SUCCESS |
| TRIANGLE 呼吸 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| SQUARE 闪烁 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| SAW_UP 扫光 | — | — | ✓ WORKING | — | — | — |
| 相位跑马（120°） | — | — | ✓ | — | ✓ | — |
| 三灯独立异色 | — | — | — | — | ✓ | — |
| 单灯（顶/底 null） | — | — | — | — | — | ✓ |
| 蜂鸣（任意） | ✓ ERROR | — | ✓ SUCCESS+ERROR | ✓ ERROR | ✓ ERROR | ✓ ERROR |

> 6 套合计覆盖：**5 种曲线全部用上**（CONSTANT/TRIANGLE/SQUARE/SAW_UP；SAW_DOWN 留待用户主题）、相位跑马、三灯独立、单灯、蜂鸣三档（静音/一声/三连）、过渡两档（0ms~1000ms）。

## 蜂鸣矩阵

| 主题 | WORKING | WAITING | SUCCESS | ERROR |
|---|---|---|---|---|
| Default | — | — | — | 2000Hz/150ms ×1，vol 60 |
| Minimal | — | — | — | —（全静音） |
| Neon | — | — | 880Hz/120ms ×1，vol 50 | 2000Hz/200ms ×3（带间隔），vol 70 |
| Nature | — | — | — | 1500Hz/120ms ×1，vol 30 |
| Aurora | — | — | — | 2000Hz/150ms ×1，vol 50 |
| Focus | — | — | — | 2000Hz/120ms ×1，vol 30 |

## 各主题状态设计速览

### Default 经典
| 状态 | 灯效 | 过渡 |
|---|---|---|
| WORKING | 三灯同相蓝呼吸 1200ms（#003366→#4A9EFF，60） | 300ms |
| WAITING | 三灯同相琥珀慢呼吸 1800ms（#3D2B00→#FFB400，50） | 300ms |
| SUCCESS | 三灯绿常亮 #00E676（70） | 500ms，hold 5s |
| ERROR | 红闪 8 次 400ms → 红常亮 + 蜂鸣一声 | 0ms，驻留 |

### Minimal 极简
| 状态 | 灯效 | 过渡 |
|---|---|---|
| WORKING | 三灯白常亮（30） | 500ms |
| WAITING | 三灯白慢呼吸 3000ms（#1A1A1A→#FFFFFF，25） | 500ms |
| SUCCESS | 三灯白常亮（55） | 500ms，hold 5s |
| ERROR | 白闪 12 次 600ms → 白常亮（90） | 0ms，驻留 |

### Neon 霓虹
| 状态 | 灯效 | 过渡 |
|---|---|---|
| WORKING | 青色 **SAW_UP 扫光跑马** 900ms，相位 0/120/240（#003333→#00FFFF，70） | 200ms |
| WAITING | 品红快呼吸 800ms（#330033→#FF00FF，60） | 200ms |
| SUCCESS | 荧光绿常亮 #39FF14（80）+ 提示音一声 | 300ms，hold 5s |
| ERROR | 红闪 12 次 250ms → 红常亮（100）+ 蜂鸣三连 | 0ms，驻留 |

### Nature 自然
| 状态 | 灯效 | 过渡 |
|---|---|---|
| WORKING | 森林绿慢呼吸 2400ms（#1A2E1A→#6B8E6B，55） | 800ms |
| WAITING | 赭棕慢呼吸 3000ms（#2E241A→#B08968，45） | 800ms |
| SUCCESS | 叶绿常亮 #7BA05B（60） | 1000ms，hold 5s |
| ERROR | 陶红闪 6 次 800ms → 常亮（70）+ 低音量一声 | 300ms，驻留 |

### Aurora 极光
| 状态 | 灯效 | 过渡 |
|---|---|---|
| WORKING | **三灯异色**（青/紫/粉）呼吸 2000ms，相位 0/120/240（60） | 600ms |
| WAITING | 三灯同相冷白呼吸 1800ms（#1A1A2E→#C5CAE9，40） | 400ms |
| SUCCESS | 三灯异色常亮（青/紫/粉，50） | 600ms，hold 5s |
| ERROR | 三灯同红闪 10 次 500ms → 常亮（80）+ 蜂鸣一声 | 0ms，驻留 |

### Focus 专注
| 状态 | 灯效 | 过渡 |
|---|---|---|
| WORKING | **仅中灯**青呼吸 2000ms（#0A1A1A→#4DD0E1，40） | 600ms |
| WAITING | 仅中灯暖黄呼吸 2600ms（#1A1400→#FFD54F，30） | 600ms |
| SUCCESS | 仅中灯绿常亮 #81C784（40） | 600ms，hold 5s |
| ERROR | 仅中灯红闪 8 次 700ms → 常亮（60）+ 低音量一声 | 0ms，驻留 |

---

## 使用与分发

- 内置：6 套编译进二进制（include_str / 资源目录），UI 主题选择器展示（名称 + 预览 + 一键试听）
- 用户主题：导入 `themes/` 目录（app config dir）；不得与内置主题同名，冲突时整体拒绝
- 兜底：默认主题 Default 作为校验失败的兜底主题（ADR-0002 T-06）

## 改进项（非阻塞）

1. **display_name 字段**：theme-format V1.0 的 `theme` 对象仅有 `name`/`version`；建议 V1.1 增加可选 `display_name`（中文展示名），缺省 = name。当前 UI 展示名由客户端映射。
2. **主题预览图**：选择器可展示灯效缩略预览（静态图或 CSS 模拟动画），需 UI 阶段设计。
3. **SAW_DOWN 曲线**：6 套内置主题未使用（语义上"渐弱"与现有状态不贴合），留待用户主题探索。
4. **静音段 volume 语义**：`volume` 合法范围为 0~100；静音间隔段（`frequency_hz = 0`）统一填 0，设备忽略其音量。

## 校验状态

- 6 个文件均已通过 theme-format V1.0 静态校验（结构/字段/引用完整性），并与 V0.4 协议参数边界对齐（period_ms ∈ [200, 5000]、频率 ∈ [100, 10000]、segment ≤ 16）。
- 运行时设备能力校验（GET_CAPABILITIES）在编译期执行，能力不符按协议返回错误。
