# MCP

`axegrid-cli mcp` is a Model Context Protocol server on stdio.

```sh
# Headless, simulated device: safe for trying things out
claude mcp add axegrid -- /path/to/axegrid-cli --simulator mcp

# Headless, real hardware: the agent connects with device.connect
claude mcp add axegrid -- /path/to/axegrid-cli mcp

# Drive the window you are looking at
axegrid --control 47800 --control-token-file ~/.axegrid-token
claude mcp add axegrid -- /path/to/axegrid-cli mcp --bridge 127.0.0.1:47800 --control-token-file ~/.axegrid-token
```

## Tools

| Tool | Does |
|---|---|
| `command_list` | Every command with parameter docs and whether it is enabled now |
| `command_run` | Run one command: `{id, params?}` |
| `command_batch` | Run up to 256 commands in order |
| `state_inspect` | The editor state the UI is showing |

Resources `axegrid://state` and `axegrid://commands` return the same JSON.

There are deliberately few tools. The surface an agent works with is the
command registry (`docs/commands.md`), discovered at run time through
`command_list`, so a new command is available to agents the moment it is
registered, with no MCP change.

A failed command comes back as a tool result with `isError: true` and a
`{kind, message}` body, so the model can read the reason and correct itself.

## Example session

```text
command_run {"id": "device.connect_simulator"}
command_run {"id": "block.select", "params": {"block": "Amp 1"}}
command_run {"id": "param.set_normalized", "params": {"param": "DISTORT_DRIVE", "position": 0.7}}
command_run {"id": "block.toggle_xy"}
state_inspect {}
```

## Caution with real hardware

`preset.store` overwrites a preset slot on the device and cannot be undone
from here. An agent connected to a real unit can run it like any other
command. See the security notes in `control-protocol.md`.
