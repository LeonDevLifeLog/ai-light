# 术语表（Glossary）

> 客户端侧术语。协议侧术语（SCENE / LedTrack / BuzzerTrack / scene_epoch / apply_mode 等）见《蓝牙通信协议 V0.4》附录 A。

| 术语 | 定义 |
|---|---|
| **接入层（L1）** | 客户端六层边界之一：接收各智能体客户端 hook 事件的层，提供标准 API 服务 |
| **标准状态集** | 抽象层的统一状态语言：`IDLE / WORKING / WAITING / SUCCESS / ERROR` 5 态，状态名开放可扩展 |
| **适配器（Adapter）** | 把某客户端专属事件翻译成标准状态的组件。第一期为独立发布的 Node.js CLI：负责安装/卸载 hooks、翻译事件并投递到 AI-Light；未来可增加 SDK 桥接或 Skill 驱动安装 |
| **运行时描述文件（runtime descriptor）** | 桌面程序写入 `~/.ailight/runtime.json` 的短期连接信息；Adapter CLI 据此发现固定回环地址与 token，用户无需理解或配置端口 |
| **source** | hook 事件中的客户端标识字段（如 `claude-code` / `cursor`），用于多源仲裁与排查 |
| **主题（Theme）** | `状态 → SCENE` 映射表，本地 JSON 配置文件，UI 可编辑，可分享/导入 |
| **hold_ms** | 主题中终态（SUCCESS/ERROR）的驻留时长；0 = 驻留到下一事件，N = N 毫秒后自动回落 IDLE |
| **仲裁（Arbitration）** | L2 业务层逻辑：多客户端/多会话竞争时决定哪个状态上屏 |
| **hook** | 客户端（Claude Code/Codex/Qoder 等）提供的生命周期事件回调机制 |
| **桥接进程（bridge）** | 常驻小进程，订阅"有 SDK 无 hooks"客户端的官方事件流并转发为标准事件（如 cursor-bridge） |
| **直接控制（direct_scene）** | 预留的高级直控通道（直接传 SCENE 参数），V2 实现，第一期不做 |
| **瞬时确认器 / 状态驻留器** | 终态两种产品语义：瞬时确认（亮几秒回 IDLE）vs 状态驻留（保持到下一事件）。本项目确定为**状态驻留器**（可配 hold_ms） |
