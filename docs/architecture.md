# Architecture

## One registry, four callers

```text
 menu / button / knob / shortcut      axegrid-cli run        control channel        MCP tool
            |                              |                       |                   |
            +------------------------------+-----------------------+-------------------+
                                           |  command id + JSON parameters
                                           v
                         Engine::run  --  enabled?  --  Args (range-checked)
                                           |
                                           v
                                   command `run` fn  -->  Session  -->  Transport  -->  device
                                           |
                                           v
                                         State   (read by the UI and by `state.inspect`)
```

`axegrid_engine::registry()` is the complete list of things the program can
do. Front ends differ only in how they pick a command and show its result.

## Layers

| Layer | Crate | Responsibility |
|---|---|---|
| L0 | `axegrid-sysex` | `Frame` (envelope, checksum), 7-bit codecs, `Request::to_frame`, `Reply::parse` |
| L1 | `axegrid-model` | Block and parameter ID tables, `Grid`, `Definitions` (cache file parser and writer) |
| L2 | `axegrid-device` | `Transport` trait, `Session` (send, wait for the matching reply, queue unsolicited messages), `MidiTransport`, `SimDevice` |
| L3 | `axegrid-engine` | `Engine`, `State`, `CommandSpec`, the registry |
| L4 | `axegrid-automation` | `dispatch` (control methods), TCP control server, `BridgeClient`, MCP |
| L4 | `axegrid-ui-egui` | `Shell`: draws `State`, collects the frame's actions, runs them as commands |

## The UI seam

`Shell::ui` builds a list of `(command id, params)` while drawing and runs
them after the frame is laid out. Menus and keyboard shortcuts are generated
from the registry. The shell holds only view state: the MIDI port pick lists,
the rename text box, the status line.

Commands run on the UI thread. A device that does not answer blocks the
window for the session timeout (1.5 s); moving the session to a worker is
listed in `docs/gaps.md`.

## The desktop control server

`axegrid --control <port> --control-token-file <path>` binds 127.0.0.1,
writes a fresh 256-bit token to the path, and accepts newline-delimited JSON.
Authenticated requests are queued to the UI thread and executed with the same
`dispatch` the headless CLI uses, so an agent acts on the very state the user
is looking at.

## Device session

`Session::exchange` sends one request and reads until a message completes it:
the reply with the request's own function ID, a success status naming it, or
the end of a list for parameter dumps. A non-zero status becomes
`SessionError::Rejected`. Everything else that arrives meanwhile (tuner,
tempo, front-panel notifications) is queued as unsolicited.

## Simulated device

`SimDevice` implements `Transport` in memory with three small presets. It
exists so that every command has an end-to-end test and so the app can be
tried without hardware. It is not a model of real parameter behaviour.
