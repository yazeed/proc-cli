---
title: "Automating process management in shell scripts"
description: "Patterns for reliable process management in shell scripts — PID files, health checks, awk parsing, and why text parsing breaks."
date: 2026-03-06
---

You need to write a deploy script. Or a dev server launcher. Or a CI cleanup step that tears down background processes after a test run. Whatever it is, your script needs to find processes, check if they're running, stop them, start new ones, and verify they came up healthy.

Here's how to do it reliably -- and where the common approaches fall apart.

## The fragile pipeline

Most people start here:

```bash
ps aux | grep myapp | grep -v grep | awk '{print $2}' | xargs kill
```

It looks reasonable. It works on your machine, in your terminal, the first time you try it. Then it breaks.

### Why it breaks

**It matches too broadly.** `grep myapp` is a substring match against the entire line, including arguments, paths, and environment. If someone is editing `/home/deploy/myapp/config.yml` in vim, that process matches. If another service has `--upstream=myapp.internal` in its command line, that matches too.

**Race conditions.** Between `ps` listing the PID and `kill` executing, the process can exit on its own. On a busy system, that PID can be reassigned to a new, unrelated process. You just killed something you didn't intend to.

**Different output across platforms.** `ps aux` output varies between macOS and Linux. Column widths shift. The COMMAND column truncates differently. If your script runs in CI on Linux and you developed it on macOS, the awk field positions might not line up.

### A real example of it going wrong

A deploy script at a startup used this to kill old app instances:

```bash
ps aux | grep "node app.js" | grep -v grep | awk '{print $2}' | xargs kill -9
```

One day, a developer was running `less /var/log/node app.js.log` in a tmux session on the deploy box. The grep matched. The deploy script killed their less process -- no real harm there. But it also matched a monitoring agent whose arguments included `--watch "node app.js"`. That monitoring agent stopped reporting, and nobody noticed the deploy had actually failed until users started complaining.

Substring matching on unstructured text is not process targeting. It's hoping for the best.

## Better patterns with standard tools

### pgrep and pkill

`pgrep` and `pkill` exist specifically to replace the `ps | grep` pattern:

```bash
# Find PIDs by process name (not substring of entire line)
pgrep myapp

# Match against the full command line when you need it
pgrep -f "node server.js"

# Exact name match only
pgrep -x myapp

# Kill by name with SIGTERM
pkill myapp

# Kill with SIGKILL
pkill -9 -f "node server.js"
```

`pgrep` matches against the process name by default, not the full command line. This avoids the "matching vim editing a config file" problem. Use `-f` when you need full command line matching, and `-x` when you need exact name matching.

In scripts, use `pgrep` to check if something is running:

```bash
if pgrep -f "node server.js" > /dev/null 2>&1; then
    echo "Server is already running"
    exit 1
fi
```

### PID files

For processes your script starts, PID files are the most reliable tracking method:

```bash
#!/bin/bash
PIDFILE="/var/run/myapp.pid"

start_app() {
    if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
        echo "Already running (PID $(cat "$PIDFILE"))"
        return 1
    fi

    ./myapp &
    echo $! > "$PIDFILE"
    echo "Started (PID $!)"
}

stop_app() {
    if [ ! -f "$PIDFILE" ]; then
        echo "No PID file found"
        return 1
    fi

    local pid
    pid=$(cat "$PIDFILE")

    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid"
        wait "$pid" 2>/dev/null
        echo "Stopped (PID $pid)"
    else
        echo "Process $pid not running (stale PID file)"
    fi

    rm -f "$PIDFILE"
}

# Clean up on exit
trap 'stop_app' EXIT INT TERM
```

`kill -0` is the key trick here: signal 0 doesn't actually send a signal, but the kernel checks if the process exists and you have permission to signal it. It's a safe "is this running?" check.

### flock for preventing duplicate instances

If your script shouldn't run concurrently with itself:

```bash
#!/bin/bash
LOCKFILE="/var/lock/myapp-deploy.lock"

exec 200>"$LOCKFILE"
if ! flock -n 200; then
    echo "Another instance is already running"
    exit 1
fi

# Rest of script runs with lock held
# Lock is released when script exits (fd 200 closes)
```

This is atomic. No race conditions. Two deploy scripts started simultaneously will not both proceed.

### Health check loops

After starting a service, don't just assume it's healthy:

```bash
start_and_wait() {
    ./myapp &
    local pid=$!
    echo $pid > "$PIDFILE"

    local retries=30
    while [ $retries -gt 0 ]; do
        if curl -sf http://localhost:3000/health > /dev/null 2>&1; then
            echo "Healthy (PID $pid)"
            return 0
        fi

        # Make sure the process hasn't crashed
        if ! kill -0 "$pid" 2>/dev/null; then
            echo "Process died during startup"
            rm -f "$PIDFILE"
            return 1
        fi

        retries=$((retries - 1))
        sleep 1
    done

    echo "Timed out waiting for health check"
    kill "$pid" 2>/dev/null
    rm -f "$PIDFILE"
    return 1
}
```

The `kill -0` check inside the loop catches the case where the process crashes immediately. Without it, you'd wait the full 30 seconds before discovering it was dead.

### wait for background process management

If your script starts multiple background processes:

```bash
#!/bin/bash
pids=()

./worker-a &
pids+=($!)

./worker-b &
pids+=($!)

./worker-c &
pids+=($!)

# Wait for all to finish, track failures
failed=0
for pid in "${pids[@]}"; do
    if ! wait "$pid"; then
        echo "Process $pid failed"
        failed=$((failed + 1))
    fi
done

if [ $failed -gt 0 ]; then
    echo "$failed process(es) failed"
    exit 1
fi
```

`wait` with a specific PID gives you the exit code of that process. `wait` without arguments waits for all children but you lose individual exit status.

## Parsing process output with awk

People reach for awk because process tools produce tabular text, and awk is the natural way to slice tabular text. Here are the patterns worth knowing.

### Common awk patterns

High CPU processes:

```bash
ps aux | awk '$3 > 80 {print $2, $11}'
```

`$3` is the CPU percentage column. This prints the PID and command of anything over 80%.

High memory processes (RSS in KB):

```bash
ps aux | awk '$6 > 500000 {print $2, $6/1024"MB", $11}'
```

`$6` is the RSS column. 500000 KB is roughly 488 MB.

All listening processes with ports (using lsof):

```bash
lsof -i -P -n | awk '/LISTEN/ {print $1, $9}'
```

Port and process from netstat:

```bash
netstat -tlnp 2>/dev/null | awk '/LISTEN/ {split($4,a,":"); print a[length(a)], $7}'
```

`split($4,a,":")` breaks the address field on colons. `a[length(a)]` gets the last element, which is the port number. This handles both `0.0.0.0:3000` and `:::3000` (IPv6).

### Why awk parsing breaks

These patterns work in interactive use. They become liabilities in scripts that run across environments.

**Column positions shift between OS versions.** macOS `ps` and Linux `ps` use the same flags but produce subtly different output. Column widths change. Extra columns appear in some configurations.

**Truncated process names.** On Linux, procps `ps` can cap the width of piped output (its man page calls the width undefined when output is not a terminal), so a long COMMAND may be cut off in cron or CI. Use `ps -ww` or `ps -o pid=,args=` in scripts. macOS `ps` does not truncate piped output.

**lsof's output is especially fragile.** Column alignment depends on the length of values in other rows. A long username or filename shifts everything.

The underlying problem is that these tools were designed for human eyes, not for programmatic consumption.

## Structured output

### The real problem with text parsing

Every awk one-liner in the previous section has implicit assumptions about column positions, field separators, and output format. These assumptions hold until they don't, and the failure mode is silent: your script extracts the wrong value and acts on it.

JSON is better for automation. The structure is explicit. Fields are named. Parsers exist in every language.

### jq patterns for process automation

`jq` is the standard tool for working with JSON on the command line:

```bash
# Extract a single field
echo '{"pid": 1234, "name": "node"}' | jq '.pid'

# Filter an array
echo '[{"pid":1,"cpu":5},{"pid":2,"cpu":90}]' | jq '.[] | select(.cpu > 50)'

# Extract into tab-separated values for further processing
echo '[{"pid":1,"name":"a"},{"pid":2,"name":"b"}]' | jq -r '.[] | [.pid, .name] | @tsv'
```

The problem is that the standard Unix process tools don't speak JSON. `ps` has no `--json` flag. `lsof` has `-F` for "field mode" output, but it's a custom format, not JSON. `netstat` and `ss` have no structured output at all.

So you're left building fragile text parsers, or wrapping them in scripts that construct JSON manually:

```bash
# This works, but look at it
ps aux | awk 'NR>1 {printf "{\"pid\":%s,\"cpu\":%s,\"mem\":%s,\"cmd\":\"%s\"}\n",$2,$3,$4,$11}'
```

That awk-to-JSON bridge is itself fragile -- it doesn't handle quotes in command names, and it still has the column position problem.

## Putting it together

Here's a real deploy script skeleton. First, the traditional version using the patterns above:

### Traditional version

```bash
#!/bin/bash
set -euo pipefail

APP_NAME="myapp"
APP_PORT=3000
APP_BIN="./target/release/myapp"
PIDFILE="/var/run/${APP_NAME}.pid"
HEALTH_URL="http://localhost:${APP_PORT}/health"
TIMEOUT=30

stop_old() {
    # Try PID file first
    if [ -f "$PIDFILE" ]; then
        local pid
        pid=$(cat "$PIDFILE")
        if kill -0 "$pid" 2>/dev/null; then
            echo "Stopping old process (PID $pid)..."
            kill "$pid"

            # Wait for graceful shutdown
            local waited=0
            while kill -0 "$pid" 2>/dev/null && [ $waited -lt 10 ]; do
                sleep 1
                waited=$((waited + 1))
            done

            # Force kill if still running
            if kill -0 "$pid" 2>/dev/null; then
                echo "Graceful shutdown timed out, sending SIGKILL..."
                kill -9 "$pid"
                sleep 1
            fi
        fi
        rm -f "$PIDFILE"
    fi

    # Also check by port in case PID file is stale
    local port_pid
    port_pid=$(lsof -i :${APP_PORT} -t 2>/dev/null | head -1)
    if [ -n "$port_pid" ]; then
        echo "Found process $port_pid still on port ${APP_PORT}, killing..."
        kill "$port_pid" 2>/dev/null
        sleep 2
        kill -9 "$port_pid" 2>/dev/null || true
    fi
}

start_new() {
    echo "Starting ${APP_NAME}..."
    $APP_BIN &
    echo $! > "$PIDFILE"
    echo "Started (PID $!)"
}

wait_healthy() {
    local retries=$TIMEOUT
    while [ $retries -gt 0 ]; do
        if curl -sf "$HEALTH_URL" > /dev/null 2>&1; then
            echo "Health check passed"
            return 0
        fi

        local pid
        pid=$(cat "$PIDFILE" 2>/dev/null)
        if [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null; then
            echo "Process died during startup"
            return 1
        fi

        retries=$((retries - 1))
        sleep 1
    done

    echo "Health check timed out after ${TIMEOUT}s"
    return 1
}

# Main
stop_old
start_new
if ! wait_healthy; then
    echo "Deploy failed"
    exit 1
fi
echo "Deploy complete"
```

This works. It handles PID files, graceful shutdown, fallback to SIGKILL, port-based detection for stale state, and health checking. But it's ~70 lines of defensive shell scripting, and the `lsof` fallback is a text-parsing step that could behave differently across environments.

### Cleaner version

```bash
#!/bin/bash
set -euo pipefail

APP_BIN="./target/release/myapp"
APP_PORT=3000
PIDFILE="/var/run/myapp.pid"
HEALTH_URL="http://localhost:${APP_PORT}/health"
TIMEOUT=30

stop_old() {
    if [ -f "$PIDFILE" ]; then
        local pid
        pid=$(cat "$PIDFILE")
        if kill -0 "$pid" 2>/dev/null; then
            echo "Stopping PID $pid..."
            kill "$pid"
            proc wait "$pid" --timeout 10 --quiet >/dev/null 2>&1 || true
            if kill -0 "$pid" 2>/dev/null; then
                kill -9 "$pid"
            fi
        fi
        rm -f "$PIDFILE"
    fi
}

start_new() {
    $APP_BIN &
    echo $! > "$PIDFILE"
    echo "Started PID $!"
}

wait_healthy() {
    local i=0
    while [ $i -lt $TIMEOUT ]; do
        if curl -sf "$HEALTH_URL" > /dev/null 2>&1; then
            return 0
        fi
        if ! kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
            return 1
        fi
        i=$((i + 1))
        sleep 1
    done
    return 1
}

stop_old
start_new
wait_healthy || { echo "Deploy failed"; exit 1; }
echo "Deploy complete"
```

Shorter, but still a meaningful amount of shell for what is fundamentally: stop old process, start new one, check it's healthy.

## proc-cli's JSON mode

If you have [proc-cli](https://github.com/yazeed/proc-cli) installed, the process inspection parts get simpler -- and structured.

Check what's on a port and get JSON back:

```bash
proc on :3000 --json | jq '.process.pid'
```

Find high-CPU processes without awk column gymnastics:

```bash
proc list --json | jq '.processes[] | select(.cpu_percent > 50) | {pid, name, cpu_percent}'
```

Check if a specific process is running by name:

```bash
if proc by myapp --json | jq -e '.count > 0' > /dev/null 2>&1; then
    echo "myapp is running"
fi
```

Kill what's on a port in a CI cleanup step:

```bash
proc kill :3000 :8080 :5432 --yes 2>/dev/null || true
```

The `--json` flag gives you named fields instead of positional columns. No awk, no column counting, no cross-platform differences in output format. And destructive commands like `kill` and `stop` support `--yes` for non-interactive use and `--dry-run` for testing.

## Install

```bash
brew install yazeed/proc/proc     # macOS
cargo install proc-cli            # Rust
npm install -g proc-cli           # npm/bun
```

See the [GitHub repo](https://github.com/yazeed/proc-cli) for all installation options.
