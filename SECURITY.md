# Security Policy

Axe-Grid sends MIDI SysEx messages to a musical instrument, parses the device's replies and a
binary definition file, and exposes its commands to scripts and AI agents through a CLI, a JSON
control channel and an MCP server. Treat device replies, files, paths, automation requests and
command parameters as potentially malicious.

## Supported versions

Axe-Grid is in early alpha. Only the latest release and the `main` branch receive fixes.

## Reporting a vulnerability

Please report privately, and give the maintainers a reasonable opportunity to investigate and
release a fix before publishing details.

Use GitHub's private reporting: on the repository page open **Security › Report a vulnerability**
([direct link](https://github.com/RamonWide/axe-grid/security/advisories/new)). Do not open a
public issue containing a working exploit, a secret, or a malicious file.

If private reporting is unavailable, open a public issue that says only that you have a security
report and need a private channel, with no technical detail.

You can expect an acknowledgement within seven days. This is a volunteer project; there is no bug
bounty.

## What to include

- the Axe-Grid version, commit or release file;
- operating system and architecture;
- the affected component (see the scope below);
- the security impact and what access an attacker needs;
- exact steps to reproduce, with expected and actual behaviour;
- crash output or logs, with personal paths and tokens removed.

## Scope

Security reports may cover:

- **Parsing:** SysEx replies from the device (`axegrid-sysex`), the definition cache file and
  `.syx` files (`axegrid-model`, `sysex.inspect_file`): crashes, hangs, memory exhaustion or
  out-of-bounds behaviour from malformed input.
- **The control channel** (`axegrid --control`): authentication bypass, token disclosure,
  acceptance of non-loopback connections, request-size or connection exhaustion.
- **The CLI, batch runner and MCP server:** command-parameter validation, and filesystem access
  through commands that take a path.
- **Device safety:** any way to make Axe-Grid overwrite a preset slot, or send a message outside
  the documented set, without the user or the calling agent explicitly asking for it.
- **Supply chain:** dependencies, CI, the release workflow, checksums and release files.

Out of scope: vulnerabilities in the Axe-Fx II firmware or in Fractal Audio's software (report
those to Fractal Audio), and anything that requires an attacker who already controls your user
account.

General bugs without a security impact belong in the normal
[issue tracker](https://github.com/RamonWide/axe-grid/issues).

## Current design and limitations

- The control channel binds to the loopback interface only and requires a 256-bit bearer token,
  written to a file the caller names, before any method is dispatched.
- An authenticated client receives the whole command surface, including commands that write to
  the device and commands that read a file path. There are no per-client capabilities and no
  filesystem roots yet.
- The control channel is not encrypted. Do not forward its port to another machine.
- MIDI itself has no authentication: any program on your computer that can open the port can talk
  to the unit, with or without Axe-Grid.
- Release files are not yet code-signed or notarized. Verify downloads against `SHA256SUMS.txt`
  and the build provenance attestation attached to each release
  (`gh attestation verify <file> --repo RamonWide/axe-grid`).
- The workspace forbids `unsafe` code, and non-test code is linted against `unwrap`, `expect` and
  `panic!`, so malformed input is expected to produce an error, not a crash. A reproducible crash
  from malformed input is a bug we want to hear about.
