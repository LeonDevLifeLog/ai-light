# @ai-light/adapter

Node.js adapter for connecting Claude Code, Codex, Qoder, TraeCode, and WorkBuddy hooks to the AI-Light desktop application.

## Requirements

- Node.js 20 or later
- A running AI-Light desktop application

## Installation

```bash
npm install --global @ai-light/adapter
```

The AI-Light desktop application normally manages installation and hook configuration. Manual installation is intended for diagnostics or recovery.

## Commands

```bash
ailight-adapter version --json
ailight-adapter doctor --json
ailight-adapter detect claude-code
ailight-adapter detect codex
ailight-adapter detect qoder
ailight-adapter detect trae
ailight-adapter detect workbuddy
```

Run hook installation through the AI-Light Integrations page so the desktop application can report errors and preserve existing tool configuration.

## License

MIT
