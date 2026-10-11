"""Regenerate docs/commands.md from the command registry.

usage: cargo run -q -p axegrid-cli -- commands | python scripts/gen-command-docs.py > docs/commands.md
"""
import json
import sys


def main() -> None:
    commands = json.load(sys.stdin)
    out = [
        "# Command reference",
        "",
        "Generated from the registry by `scripts/gen-command-docs.py`. Do not edit by hand;",
        "regenerate after adding or changing a command (see `CONTRIBUTING.md`).",
        "",
        f"{len(commands)} commands.",
        "",
    ]
    group = None
    for command in commands:
        namespace = command["id"].split(".")[0]
        if namespace != group:
            group = namespace
            out += [f"## {namespace}", ""]
        out += [f"### `{command['id']}`", ""]
        extra = []
        if command["menu"]:
            extra.append("Menu: " + command["menu"].replace("/", " > ") + ".")
        if command["shortcut"]:
            extra.append("Shortcut: " + command["shortcut"] + ".")
        out += [" ".join([command["doc"]] + extra), ""]
        if command["params"]:
            out += ["| Parameter | Type | Required | Meaning |", "|---|---|---|---|"]
            for param in command["params"]:
                kind = param["type"].replace("|", " or ")
                required = "yes" if param["required"] else "no"
                out.append(f"| `{param['name']}` | {kind} | {required} | {param['doc']} |")
            out.append("")
    sys.stdout.write("\n".join(out))


main()
