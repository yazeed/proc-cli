---
title: "Managing child processes in Node.js"
description: "Node.js child process methods explained: exec vs execFile vs spawn vs fork. Signal handling, orphan prevention, and graceful shutdown patterns."
date: 2026-03-10
---

Node is single-threaded, so anything CPU-bound blocks the event loop. Child processes are how you run external commands, parallelize work, and shell out to other tools without freezing your server.

Node gives you four methods. They look similar but differ in important ways.

## exec: buffered output, shell interpretation

```javascript
const { exec } = require('child_process');

exec('ls -la | grep node', (error, stdout, stderr) => {
  if (error) {
    console.error(`exit code: ${error.code}`);
    return;
  }
  console.log(stdout);
});
```

`exec` spawns a shell (`/bin/sh` on Unix), runs your command string in it, buffers the entire output, and returns it in the callback.

**When to use it:** Short commands where you want all the output at once. Shell features like pipes, redirects, and globbing work because it runs in a real shell.

**Risks:**

- **Command injection.** If you interpolate user input into the command string, you have a shell injection vulnerability. Never do `exec(\`ls ${userInput}\`)`.
- **Buffer overflow.** The default `maxBuffer` is 1 MB. If the command produces more output, you get `ERR_CHILD_PROCESS_STDIO_MAXBUFFER`. Increase it with `{ maxBuffer: 10 * 1024 * 1024 }` or use `spawn` instead.

## execFile: no shell, safer

```javascript
const { execFile } = require('child_process');

execFile('git', ['log', '--oneline', '-10'], (error, stdout, stderr) => {
  console.log(stdout);
});
```

Like `exec` but **does not spawn a shell**. Arguments are passed as an array, not a string. This means:

- No shell injection risk
- No pipes, redirects, or globbing
- Slightly faster (no shell startup overhead)

Use `execFile` when running a known binary with known arguments. Use `exec` only when you genuinely need shell features.

## spawn: streaming output

```javascript
const { spawn } = require('child_process');

const child = spawn('find', ['.', '-name', '*.js']);

child.stdout.on('data', (data) => {
  console.log(`stdout: ${data}`);
});

child.stderr.on('data', (data) => {
  console.error(`stderr: ${data}`);
});

child.on('close', (code) => {
  console.log(`exited with code ${code}`);
});
```

`spawn` is the low-level primitive. Output is streamed as it arrives rather than buffered. No shell unless you pass `{ shell: true }`.

**When to use it:** Long-running processes, large output, or when you need to process output as it arrives. Building CLI tools, log tailers, or anything that pipes data.

### stdio options

```javascript
// pipe: parent reads/writes to child (default)
spawn('cmd', [], { stdio: 'pipe' });

// inherit: child uses parent's stdin/stdout/stderr
spawn('cmd', [], { stdio: 'inherit' });

// ignore: discard all I/O
spawn('cmd', [], { stdio: 'ignore' });

// mix them: [stdin, stdout, stderr]
spawn('cmd', [], { stdio: ['ignore', 'pipe', 'inherit'] });
```

`inherit` is useful when you want the child's output to appear directly in the parent's terminal — like running a build command and seeing its output in real time.

## fork: node-to-node IPC

```javascript
// parent.js
const { fork } = require('child_process');
const child = fork('./worker.js');

child.send({ task: 'process-data', payload: [1, 2, 3] });

child.on('message', (result) => {
  console.log('worker result:', result);
});

// worker.js
process.on('message', (msg) => {
  const result = msg.payload.map(n => n * 2);
  process.send({ result });
});
```

`fork` is `spawn` specialized for Node scripts. It automatically creates an IPC (Inter-Process Communication) channel between parent and child. Both sides can use `send()` and `on('message')` to exchange structured data.

**When to use it:** CPU-intensive work you want to offload to another Node process. Image processing, data parsing, compilation — anything that would block the event loop.

`fork` always spawns a new Node process. For running non-Node commands, use `spawn` or `exec`.

## signal handling and graceful shutdown

When your Node process receives SIGTERM, child processes don't automatically get notified. You need to forward the signal:

```javascript
const children = [];

function spawnWorker() {
  const child = spawn('node', ['worker.js']);
  children.push(child);

  child.on('exit', (code, signal) => {
    children.splice(children.indexOf(child), 1);
  });

  return child;
}

// graceful shutdown
process.on('SIGTERM', () => {
  console.log('SIGTERM received, stopping workers');

  for (const child of children) {
    child.kill('SIGTERM');
  }

  // give workers time to clean up, then force exit
  setTimeout(() => {
    for (const child of children) {
      child.kill('SIGKILL');
    }
    process.exit(0);
  }, 5000);
});
```

**Key point:** `process.on('SIGTERM')` prevents the default behavior (immediate exit). You must call `process.exit()` yourself after cleanup, or the process won't stop.

## orphan prevention

If the parent Node process is killed with `kill -9` (SIGKILL), it doesn't get a chance to run signal handlers. Child processes become orphans — reparented to PID 1 (init) and keep running.

This is the most common source of "ghost" Node processes eating CPU in the background. Dev servers that spawn watchers, build tools that spawn compilers — all orphaned when you `Ctrl+C` too aggressively or when the parent crashes.

## detached processes

Sometimes you *want* the child to outlive the parent:

```javascript
const child = spawn('node', ['server.js'], {
  detached: true,
  stdio: 'ignore'
});

child.unref();  // let parent exit without waiting for child
```

`detached: true` puts the child in a new session and process group (on Windows it gets its own console window instead). `unref()` tells the parent's event loop not to wait for this child. The parent can exit and the child keeps running.

Without `unref()`, the parent's event loop stays alive waiting for the detached child to exit.

## common pitfalls

### ENOENT

```
Error: spawn mycommand ENOENT
```

The command doesn't exist on `PATH`, or you forgot to set `shell: true` when using shell builtins. This error is emitted on the `error` event, not `exit`:

```javascript
const child = spawn('nonexistent-command');
child.on('error', (err) => {
  // err.code === 'ENOENT'
});
```

If you don't listen for `error`, it throws an unhandled exception and crashes your process.

### maxBuffer exceeded

```javascript
// default is 1MB — increase for large output
exec('find / -name "*.log"', { maxBuffer: 50 * 1024 * 1024 }, callback);

// or better: use spawn and stream
const child = spawn('find', ['/', '-name', '*.log']);
```

### not handling the error event

Every child process can emit `error`. If you don't listen for it, Node throws an unhandled exception:

```javascript
const child = spawn('some-command');
// ALWAYS add this:
child.on('error', (err) => {
  console.error('child process error:', err.message);
});
```

## with proc-cli

When Node child processes escape cleanup, `proc tree <pid>` shows that process and everything it spawned (add `-a` to see its ancestors). `proc by node --in .` finds all Node processes in your project. And `proc stop :3000 --yes` handles the "port still in use after crash" scenario.

```bash
proc tree 12345            # see process tree from a PID
proc by node --in .        # node processes in current project
proc stop :3000 --yes      # clean up port after crash
```

## Install

```bash
brew install yazeed/proc/proc     # macOS
cargo install proc-cli            # Rust
npm install -g proc-cli           # npm/bun
```

See the [GitHub repo](https://github.com/yazeed/proc-cli) for all installation options.
