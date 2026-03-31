# RRAI — Rust Remote AI

Discord-based Claude Code agent controller. Each Discord channel runs an isolated Claude Code session with its own workspace, tool approvals, and message queue.

## Environment Variables

| Variable | Required | Description |
|---|---|---|
| `DISCORD_BOT_TOKEN` | Yes | Discord bot authentication token |
| `DISCORD_GUILD_ID` | Yes | Discord server ID (numeric) |
| `ALLOWED_USER_IDS` | Yes | Comma-separated Discord user IDs |
| `RRAI_DATA_DIR` | No | Data directory (default: `~/.rrai`) |
| `RATE_LIMIT_PER_MINUTE` | No | Per-user rate limit (default: 10) |

## Features

### Session Management

- **Per-channel sessions** — each Discord channel maps to an isolated Claude Code subprocess with its own working directory (`~/.rrai/sessions/<channel_id>`)
- **Session resume** — sessions persist across bot restarts via stored session IDs
- **Real-time streaming** — Claude responses stream into Discord with throttled edits (1.5s interval)
- **Heartbeat** — sends a heartbeat indicator if no output for 15 seconds
- **Stale session recovery** — detects and replaces dead sessions automatically
- **Process lock** — prevents duplicate bot instances via `.bot.lock` PID file

### Slash Commands

| Command | Description |
|---|---|
| `/start-new` | Create a new channel + Claude session with an initial message |
| `/stop` | Stop the active Claude session in the current channel |
| `/status` | Show all projects and session statuses for the server |
| `/sessions` | Browse, resume, or delete past sessions (up to 24 listed) |
| `/clear` | Delete all session files and bulk-clear channel messages |
| `/last` | Show the last Claude response from the current session |
| `/queue list` | Show queued messages with remove buttons |
| `/queue clear` | Clear all queued messages |
| `/model` | Set Claude model per channel (Opus, Sonnet, Haiku, Default) |
| `/models` | List available Claude models |
| `/context` | Show context window usage and CLAUDE.md size |
| `/usage` | Usage tracking info |
| `/auto-approve` | Toggle automatic tool approval for the channel |
| `/mcp` | Toggle MCP servers on/off (with autocomplete) |
| `/skills` | List available Claude Code slash commands |
| `/agents` | List available Claude agent types |
| `/config` | Show current bot configuration |

### Tool Approval Workflow

- **Auto-allow** read-only tools: `Read`, `Glob`, `Grep`, `WebSearch`, `WebFetch`, `TodoWrite`
- **Manual approval** for write tools via Discord buttons (Approve / Deny / Auto-approve All)
- **Per-channel auto-approve** toggle — bypass approval for all tools
- **5-minute timeout** on approval requests

### Question Answering (AskUserQuestion)

- Single-select with button UI
- Multi-select with dropdown menu
- Custom text input option
- Multi-question flows with progress indicator

### Message Queue

- Queues up to 5 messages when a session is busy
- Confirmation UI before adding to queue
- Remove individual items or clear all
- Auto-processes next queued message on completion

### File Attachment Handling

- Downloads Discord attachments to `.claude-uploads/` per channel
- Image detection (PNG, JPG, GIF, WEBP) — appended to prompt
- File type blocking for dangerous extensions (EXE, BAT, DLL, etc.)
- 25MB file size limit
- Path traversal protection

### MCP Server Management

- Per-channel MCP server toggling via `/mcp` command
- Live toggle without restarting session (SDK control)
- Reads server list from project `.mcp.json`
- Disabled servers stored in database

### Per-Channel Model Selection

- Set model per channel: Opus, Sonnet, Haiku
- Session restarts with new model on change
- Stored in database, persists across restarts

### Output Formatting

- Smart message splitting at 1900 chars with code fence preservation
- Result embeds with token usage (input/output), duration, and cost
- Tool approval embeds with contextual formatting (file edits, bash commands)
- Interactive components: buttons, select menus, embeds

### Security & Rate Limiting

- User whitelist authentication on all interactions
- Per-user sliding window rate limiting
- File upload security (type blocking, size limits, path traversal prevention)
- Dangerous extension blacklist

### Database (SQLite)

- **Projects** — channel-to-workspace mapping, model, auto-approve, disabled MCPs
- **Sessions** — session IDs, status tracking (Online/Offline/Waiting/Idle), last activity
- WAL mode with foreign key constraints
- Schema migration system via `PRAGMA user_version`
- Indexed lookups on `channel_id` and `guild_id`
- CHECK constraints on status values

### Cleanup & Maintenance

- Orphaned project detection (1-hour cycle)
- Removes `.claude-uploads` and Claude session directories for deleted channels
- Cascading database cleanup on project removal

## Architecture

```
Discord Channel
    → Message Handler (auth, rate limit, attachments)
    → Session Manager (ensure session, queue if busy)
    → Claude Process (CLI subprocess with JSON streaming)
    → Output Formatter (split, embed, buttons)
    → Discord Response (stream edits, result embed)
```

Each channel's Claude process runs with `cwd` set to `~/.rrai/sessions/{channel_id}`, giving full filesystem isolation between sessions.

## Required Discord Permissions

### Gateway Intents

| Intent | Privileged | Why |
|---|---|---|
| `GUILDS` | No | Receive guild events, channel lookups, orphan cleanup |
| `GUILD_MESSAGES` | No | Receive messages to forward to Claude sessions |
| `MESSAGE_CONTENT` | Yes | Read message text (required for non-slash-command input) |

### Bot Permissions

| Permission | Used By |
|---|---|
| Send Messages | All responses, streaming output, embeds, approval UI |
| Manage Messages | `/clear` bulk message deletion |
| Manage Channels | `/start-new` channel creation |
| Read Message History | `/clear` message fetching, attachment downloads |
| Embed Links | Result embeds, tool approval embeds, status displays |
| Attach Files | File outputs from Claude sessions |
| Add Reactions | Message receipt confirmation |
| View Channels | All channel operations (implicit) |

### Setup

1. Enable **Message Content Intent** in the Discord Developer Portal under Bot settings
2. Generate an invite URL with the bot permissions above (integer: `126032`)
