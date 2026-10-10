#!/usr/bin/env python3
"""PreToolUse hook for Bash: rejects commands that break AGENTS.md rules 2, 4 and 15.

Exit 2 blocks the call; the message on stderr goes back to the agent.
"""
import json
import re
import sys

call = json.load(sys.stdin)
tool = call.get("tool_input", {})
# Heredoc bodies are data (scripts, file contents), not commands.
cmd = re.sub(
    r"<<-?\s*(['\"]?)(\w+)\1[^\n]*\n.*?\n\s*\2\s*(?=\n|$)",
    lambda m: m.group(0).split("\n", 1)[0],
    tool.get("command", ""),
    flags=re.S,
)
problems = []

WRAPPERS = {
    "nohup", "env", "exec", "setsid", "timeout", "time", "gdb", "perf", "strace", "valgrind", "sudo", "--",
    "tools/slice", "systemd-run", "if", "then", "else", "elif", "while", "until", "do", "!",
}


def commands(text):
    """The words of each simple command, wrappers and VAR=value assignments removed."""
    for segment in re.split(r"\|\||&&|[;&|()\n]|\$\(|`", text):
        words = [w for w in segment.split() if not re.fullmatch(r"[A-Za-z_]\w*=\S*", w)]
        while words:
            if words[0] == "flock":
                del words[:2]
            elif words[0] in WRAPPERS or words[0].startswith("-") or re.fullmatch(r"\d+[smh]?", words[0]):
                words.pop(0)
            else:
                break
        if words:
            yield words
    # Scripts given to `bash -c` / `sh -c` are commands too.
    for inner in re.finditer(r"\b(?:bash|sh)\s+-c\s+(['\"])(.*?)\1", text, flags=re.S):
        yield from commands(inner.group(2))


in_slice = re.search(r"tools/slice|--slice=void-agent\.slice|flock /tmp/void-build\.lock", cmd)

for words in commands(cmd):
    name = words[0].rsplit("/", 1)[-1]
    # Rule 2: processes are found and handled by numeric PID only.
    if name in {"pgrep", "pkill", "killall", "pidof"}:
        problems.append(
            f"rule 2: no `{name}`. Take the PID from `$!` right after starting the program, or have a "
            "script write it to a file, then use `kill -0 <PID>` / `ps -p <PID>` / `kill <PID>`."
        )
    # Rule 4: builds and the game run in void-agent.slice, builds with -j 8.
    if name == "cargo" and len(words) > 1 and words[1] in {"build", "test", "run", "check", "clippy", "bench"}:
        line = " ".join(words)
        if not in_slice:
            problems.append(f"rule 4: `{line}` must run through `tools/slice` (see guides/build.md).")
        elif not re.search(r"-j\s*8\b", line):
            problems.append(f"rule 4: `{line}` must use `-j 8`.")
    if name == "void-app" and not in_slice:
        problems.append(
            "rule 4: run the game inside void-agent.slice "
            "(`systemd-run --user --scope --quiet --slice=void-agent.slice -- ...`, see guides/computeruse.md)."
        )

if re.search(r"(^|[;&(\n|]\s*)ps\b[^|;&\n]*\|\s*[ef]?grep\b", cmd):
    problems.append(
        "rule 2: no `ps | grep` to find processes. Take the PID from `$!` or a PID file, "
        "then use `ps -p <PID>` / `kill -0 <PID>`."
    )

# Rule 15: a foreground call waits at most about two minutes.
timeout_ms = tool.get("timeout")
if not tool.get("run_in_background") and timeout_ms is not None and timeout_ms > 150_000:
    problems.append(
        f"rule 15: foreground timeout {timeout_ms} ms is too long. Start long work with "
        "run_in_background and come back with waits of about 90 s that also check `kill -0 <PID>`."
    )
for words in commands(cmd):
    if words[0] == "sleep" and len(words) > 1 and re.fullmatch(r"\d+", words[1]) and int(words[1]) > 10:
        problems.append(
            f"rule 15: `sleep {words[1]}` blocks. Wait with `timeout 90 tail --pid=<PID> -f /dev/null` "
            "or a loop that also checks `kill -0 <PID>`, or run the work in the background."
        )
        break

# Rule 15: a program left running must not hold the tool's stdin/stdout/stderr.
unquoted = re.sub(r"'[^']*'|\"(?:\\.|[^\"\\])*\"", "''", cmd)
for line in unquoted.split("\n"):
    for amp in re.finditer(r"(?<![&>|<])&(?=\s|$)", line):
        before = line[: amp.start()]
        if re.search(r"<\s*/dev/null", before) and re.search(r">\s*\S", before) and "2>&1" in before:
            continue
        problems.append(
            "rule 15: a background `&` must detach all three streams: "
            "`nohup <cmd> > <log> 2>&1 < /dev/null &`, or use run_in_background."
        )
        break

if problems:
    print("Blocked by .claude/hooks/bash-guard.py:\n- " + "\n- ".join(dict.fromkeys(problems)), file=sys.stderr)
    sys.exit(2)
