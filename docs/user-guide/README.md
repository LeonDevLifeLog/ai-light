# AI Light 用户手册

AI Light 把 Claude Code、Codex 等 AI 编程工具的运行状态同步到桌面应用和 AI Light 状态灯。你可以不用反复查看终端，就知道任务正在工作、等待输入、已经完成，还是遇到了错误。

本手册面向 AI Light 的日常使用者。第一次使用时，请从[快速开始](./getting-started.md)开始。

## 手册导航

| 你想完成的事情 | 阅读内容 |
|---|---|
| 第一次安装、连接并让状态灯亮起来 | [快速开始](./getting-started.md) |
| 看懂状态、托盘和日常操作 | [日常使用](./daily-use.md) |
| 查找、连接、断开或更换设备 | [设备管理](./devices.md) |
| 连接 Claude Code、Codex、Qoder 或 WorkBuddy | [接入 AI 编程工具](./integrations.md) |
| 切换、创建或导入灯效主题 | [主题与提示音](./themes.md) |
| 调整显示规则、外观和开机自启 | [个性化设置](./settings.md) |
| 设备不亮、连接失败或状态不更新 | [故障排查](./troubleshooting.md) |
| 更新、卸载、清理数据和了解隐私边界 | [维护与数据管理](./maintenance.md) |
| 查找术语或高级功能 | [参考资料](./reference/README.md) |

## AI Light 如何工作

```text
Claude Code / Codex
        ↓ 任务状态
   AI Light 应用
        ↓ 蓝牙
 AI Light 状态灯
```

AI Light 应用在本机接收任务状态，再根据当前主题把状态转换成灯光和提示音。日常使用不需要配置网络地址或编辑 JSON 文件。

## 使用前需要准备

- 一台已供电的 AI Light 设备；
- 一台蓝牙可用的 macOS、Windows 或 Linux 电脑；
- AI Light 安装包；
- 如需自动同步任务状态，安装受支持的 Claude Code、Codex、Qoder 或 WorkBuddy；
- 连接 AI 编程工具时，电脑需要可用的 Node.js 和 npm。

安装包格式、支持的最低系统版本和发布说明可能随版本变化，请以你获得安装包时附带的发布说明为准。

## 名称说明

- **AI Light**：整个产品；
- **AI Light 应用**：安装在电脑上的桌面软件；
- **AI Light 设备 / AI Light 状态灯**：通过蓝牙连接的实体设备；
- **AI 编程工具**：Claude Code、Codex 等产生任务状态的工具。
