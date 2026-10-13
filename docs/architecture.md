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
                                         State   (shown by the UI, returned by `state.inspect`)
```

`axegrid_engine::registry()` is the complete list of things the program can
do. Front ends differ only in how they pick a command and show its result.

## Layers

| Layer | Crate | Responsibility |
|---|---|---|
| L0 | `axegrid-sysex` | `Frame` (envelope, checksum), 7-bit codecs, `Request::to_frame`, `Reply::parse` |
| L1 | `axegrid-model` | Block and parameter ID tables, `Grid`, `Definitions` (cache file parser and writer) |
| L2 | `axegrid-device` | `Transport` trait, `Session` (send, wait for the matching reply, queue unsolicited messages), `MidiTransport`, `SimDevice` |
| L3 | `axegrid-engine` | `Engine`, `State`, `CommandSpec`, the registry; `Worker` and `View` for front ends with a window |
| L4 | `axegrid-automation` | `dispatch` (control methods), TCP control server, `BridgeClient`, MCP |
| L4 | `axegrid-ui-egui` | `Shell`: draws `State`, collects the frame's actions, runs them as commands |

## The UI seam

`Shell::ui` builds a list of `(command id, params)` while drawing and hands
them over after the frame is laid out. Menus and keyboard shortcuts are
generated from the registry. The shell holds only view state: the MIDI port
pick lists, the rename text box, the status line.

## The engine's own thread

Talking to a device means waiting for it: up to the session timeout (1.5 s)
for each request it does not answer. So the window does not own the engine.
`axegrid_engine::Worker` does, on a thread named `device-session`:

```text
 UI thread                                   device-session thread
 ---------                                   ---------------------
 Shell::run(id, params)  -- job -->  queue -->  Engine::run(id, params)
                                                 publish View (a copy of State,
                                                 the editor pages, what is enabled)
 Shell::settle(outcome)  <-- report --           wake the window
 draws from Worker::view()
```

- Jobs run one at a time, in the order they were handed over. The control
  channel's requests join the same queue.
- After every job the worker publishes a `View`. The window draws from the
  latest one and never touches the engine, so no frame waits for the device.
- A job's report runs after its view is published: whoever hears that a
  command finished already sees what it did.
- While a control is dragged it issues a set every frame. The shell sends one
  at a time for each parameter and keeps only the newest one waiting, and
  shows the value asked for until the device has answered.
- Housekeeping (listening for the tuner and tempo, the load meter, names for
  the managers, choice lists) is only handed over when nothing else is
  waiting, so it never delays an edit by more than one short job.

`Headless` (the CLI, MCP without a window) still owns an `Engine` directly
and runs commands where it is called.

## Undo history

The device holds the truth, so the history does not copy presets. Each
entry records what one edit changed on the device: a parameter's raw value
before and after, an X/Y state, the preset name, a modifier field, or the
grid before and after. A pasted block is remembered as its settings before
and after, and a pasted scene as the group of changes it made, undone in
reverse. Undo sends the old value back; for the grid it places blocks and
draws cables until the device's grid matches the remembered one, lifting
shunts first so that no shunt ID is ever in two cells. Edits to one
parameter that follow each other within 0.6 s are one entry. The history
belongs to the edit buffer: loading a preset, importing one or losing the
device empties it. Commands that edit the preset record through
`Engine::record_param`, `record_xy`, `record_rename` and `recording_grid`
(`history_cmds.rs`).

## What lives on the computer

Almost nothing: the device holds the preset. The exceptions are the two
clipboards (`block.copy`, `scene.copy`), which last as long as the engine,
and scene names, which the device has no place for. A front end a person
uses builds its engine with `Engine::for_this_computer()`, which keeps them
in `scene-names.json` under the system's application-data folder; an engine
made with `Engine::new()`, as the tests make it, writes nothing.

## Modifiers

A modifier belongs to a parameter and has thirteen fields the editor shows
(`modifier_cmds.rs`). `modifier.open` reads them and keeps them as the open
modifier, which `View::modifier` hands to the window; the other modifier
commands act on the open one unless told otherwise. The device names a
source only when a modifier uses it, so the engine collects the names it has
seen. Which parameters take a modifier, and through which parameter it is
addressed, comes from `layouts::modifier_param`; a unit does not say. The selector is range-checked twice on the way out, in the request and
in the session, because a unit asked for a field past the last one stops
answering (see [gaps](gaps.md)).

## Choice lists

The names in a list control (amp models, effect types, cabs) are read from
the device and kept until it disconnects, keyed by effect type and parameter
so that two instances of a block share them (`lists_cmds.rs`). A parameter
that reports a highest choice index has a list of its own, read with one
request (function `28`). The cab block's cab picker has none; its value is a
position in the device's cab list, which is read name by name (function
`12`). `block.read_lists` takes a budget of requests, so the window can read
a little at a time between the user's edits.

## The desktop control server

`axegrid --control <port> --control-token-file <path>` binds 127.0.0.1,
writes a fresh 256-bit token to the path, and accepts newline-delimited JSON.
Authenticated requests are handed to the engine's thread and executed with
the same `dispatch` the headless CLI uses, so an agent acts on the very state
the user is looking at, in turn with the user's own clicks.

## Device session

`Session::exchange` sends one request and reads until a message completes it:
the reply with the request's own function ID, a success status naming it, or
the end of a list for parameter dumps. A non-zero status becomes
`SessionError::Rejected`. Everything else that arrives meanwhile (tuner,
tempo, front-panel notifications) is queued as unsolicited.

## Simulated device

`SimDevice` implements `Transport` in memory with three small presets. It
exists so that every command has an end-to-end test and so the app can be
tried without hardware. It is not a model of real parameter behaviour: every
list has the same three choices, for one.
