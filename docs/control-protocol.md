# Control protocol

Newline-delimited JSON. The same four methods are available three ways:

| Transport | Start it | Acts on |
|---|---|---|
| stdio | `axegrid-cli serve` | an engine inside the CLI process |
| TCP, loopback | `axegrid --control <port> --control-token-file <path>` | the running desktop window |
| MCP | `axegrid-cli mcp` (see `mcp.md`) | either of the above |

## Messages

```json
{"id": 1, "method": "command.run", "params": {"id": "preset.next"}}
{"id": 1, "result": {"number": 1, "name": "Crunch", "scene": 1}}
{"id": 2, "method": "command.run", "params": {"id": "scene.select", "params": {"scene": 99}}}
{"id": 2, "error": {"kind": "bad_param", "message": "parameter \"scene\": 99 is above the maximum 8"}}
```

| Method | Params | Result |
|---|---|---|
| `command.list` | none | every command: `id`, `label`, `menu`, `shortcut`, `doc`, `enabled`, `params` |
| `command.run` | `{id, params?}` | the command's result |
| `command.batch` | `{steps: [{id, params?}], stop_on_error?}` | `{completed, of, results: [{ok, result \| error}]}`; at most 256 steps; stops at the first failure unless `stop_on_error` is `false` |
| `state.inspect` | none | connection, preset, grid, block states, selected block and its parameters |

Error `kind` is one of `unknown_command`, `disabled`, `bad_param`, `device`,
`io`, `not_found`, `bad_request`, `unauthorized`, `transport`.

## TCP specifics

The first line on every connection must authenticate:

```json
{"id": "auth", "method": "auth", "params": {"token": "<64 hex digits>"}}
```

Anything else, or a wrong token, gets one `unauthorized` error and the
connection is closed. The app writes a fresh token to the file you name each
time it starts. With port `0` the app picks a free port and prints
`control channel listening on 127.0.0.1:<port>`.

## Security posture

Implemented: binds IPv4 loopback only; the bridge client refuses non-loopback
addresses; token comparison is constant-time; request lines are capped at
1 MiB; at most 16 connections; 30 s I/O timeouts; batches capped at 256.

Not implemented: encryption, per-command capabilities, or a filesystem
sandbox. The token grants the whole command registry, including commands
that read a file path you pass (`definitions.load`, `sysex.inspect_file`) and
commands that overwrite presets on the device. Keep the token file private,
enable `--control` only for a session you trust, and do not forward the port.
