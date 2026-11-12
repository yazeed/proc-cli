---
layout: page
title: proc list
description: "All processes, or the ones named."
permalink: /commands/list
---

With filters for name, directory, CPU, memory, uptime, status and parent. It only reads. Aliases: `l`, `ps`.

```bash
proc list                                 # all processes
proc list node python                     # the processes named node or python
proc list node --cmd                      # ... or with node anywhere in their command line
proc list --sort cpu --limit 10           # the 10 busiest
proc list --in . --min-mem 100            # processes in this project using over 100 MB
proc list --status running --sort mem     # running processes, biggest first
```

## Output

```text
✓ Found 2 processes

 PID    NAME     USER  CPU%     MEM  UPTIME  COMMAND
 39158  python3  dev    0.0  26.8MB     13s  python3 -m http.server 0 --bind 127.0.0.1
 38120  zsh      dev    0.0   4.1MB  2h 14m  -zsh
```

Every process table in proc (`list`, `by`, `in`, `for` with several hits, `orphans`, `stuck`, `watch`, and the list in front of a `--dry-run` or a question) has these columns: PID, NAME, USER, CPU%, MEM, UPTIME and COMMAND. A value proc can't read shows as `-`. With `--in`, the directory is the second column (`PID DIR NAME ...`).

One process is always one line. COMMAND takes the width the other columns leave and ends with `…` where it is cut. The directory under `--in` is cut in the middle (`/Users/…/my-app/.next/standalone`) before it takes more than a third of the line. The list in front of a `--dry-run` or a question prints the command line whole, since that is what you approve.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--cmd` | | Match the names as a substring of the process name or full command line (like `pkill -f`) |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--path <path>` | `-p` | Filter by executable path |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--status <s>` | | Filter by status: running, sleeping, stopped, zombie, idle (Linux), starting (macOS, FreeBSD, NetBSD), disk-wait (Linux; `disk_wait` also works) |
| `--min-uptime <s>` | | Minimum uptime in whole seconds |
| `--parent <pid>` | | Only children of this parent PID |
| `--sort <key>` | `-s` | Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID (default: cpu) |
| `--limit <n>` | `-n` | Limit results |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table, and cut nothing |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Details

- **Status** is `running`, `sleeping`, `stopped`, `zombie`, `dead` or `unknown`, and three more where the system tells them apart: `idle` (Linux: an idle kernel thread), `starting` (macOS, FreeBSD and NetBSD: a process still being created by `fork`) and `disk wait` (Linux: uninterruptible sleep on the disk). JSON writes it `disk_wait`, and `--status` takes `disk-wait` or `disk_wait`. `dead` and `unknown` only appear in output.
- **Several names** are separate arguments. Comma-separated lists (`node,python`) still work but are deprecated.

## When nothing matches

`⚠ No processes found`, exit 0: an empty list is still an answer. A name that no process has but some command lines do gets a line saying how many (`1 process has 'serve' in its command line (add --cmd to match those)`) and `Try: proc by serve --cmd to see them`. [Messages](messages) explains each message.

## JSON

```text
{
  "action": "list", "success": true, "count": 2,
  "processes": [<Process>, ...]
}
```

Each process has all the table shows and more: `parent_name`, `child_count`, `cpu_time_secs` and `open_files` (a key is left out where the system doesn't give the value), and `"runs_proc": true` on a process proc runs under (its shell, or the agent that ran it). Nothing found is `count: 0` and `processes: []`. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#process-and-port-objects).

## See also

[by](by) and [in](in) to start from a name or a directory, [watch](watch) for a live table.
