---
title: "umount: target is busy (device or resource busy): find what's using it"
description: "Why umount says \"target is busy\" and rm says \"Device or resource busy\", and how to find the process using it on Linux, macOS and Windows."
date: 2026-11-12
---

You try to unmount a drive and get:

```
umount: /mnt/usb: target is busy.
```

Or `rm` refuses with `rm: cannot remove 'data': Device or resource busy`. Both are the same error underneath, `EBUSY`: the system won't remove something a process is still using. Find that process, and the error goes away.

## The short answer

```bash
# everything using that mount
sudo lsof /mnt/usb

# the same, with fuser (Linux)
sudo fuser -vm /mnt/usb
```

Then, for each process it lists: if it's a shell sitting in the mount, `cd` out of it; if it's a program, quit it or `kill` its PID. Run `umount` again.

Most of the time it's your own terminal: a shell whose current directory is inside the mount keeps it busy, even if it's doing nothing.

## What "busy" means

Linux won't unmount a filesystem while any process holds something on it. That can be:

- an **open file**: an editor, a log being written, a `tail -f`, a database;
- a **working directory**: a shell or program whose current directory is inside the mount;
- a **running program** stored there, or a library it loaded from there;
- **another mount** inside it, a **swap file** on it, or a **loop device** backed by a file on it.

The same rule makes `rm` and `rmdir` fail on a directory that's a mount point: unmount it first.

## Find the process

`lsof` given a **mount point** lists every open file on that filesystem, along with the processes whose working directory is on it:

```bash
sudo lsof /mnt/usb
```

```
COMMAND   PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
bash     4120  dev  cwd    DIR    8,1     4096    2 /mnt/usb
vim      4388  dev    3u   REG    8,1    12288   14 /mnt/usb/notes.txt
```

`cwd` in the FD column is a working directory: that's the shell sitting in the mount. Use `sudo`, or you'll only see your own processes.

For a directory that isn't a mount point, `lsof` looks at that one path only. `lsof +D /path` searches everything below it, which can be slow on a big tree.

`fuser` gives the same answer more briefly. `-m` means "anything on this mount", `-v` shows names:

```bash
sudo fuser -vm /mnt/usb
```

```
                     USER        PID ACCESS COMMAND
/mnt/usb:            root     kernel mount /mnt/usb
                     dev        4120 ..c.. bash
                     dev        4388 F.... vim
```

In `ACCESS`, `c` is a working directory, `f` an open file (`F` when it's open for writing), `e` a running program and `m` a mapped file or library. `fuser -km /mnt/usb` kills every process it lists, without asking. That includes your shell, so look before you use it.

## Make it un-busy

1. **Shells:** `cd ~` in each terminal that's inside the mount, including terminals in your editor.
2. **Programs:** quit them normally, or `kill <PID>` (`kill -9` if one won't exit).
3. **Services** (a database, Docker, a file indexer): stop them through their manager, or they'll come back: `sudo systemctl stop <name>`.
4. **Then:** `sudo umount /mnt/usb`.

If you can't stop what holds it, `sudo umount -l /mnt/usb` (lazy) detaches the mount now and finishes when the last user lets go. The processes keep their files open, so don't unplug the drive until they've exited. For a dead network share, `umount -f` forces it.

## rm: cannot remove: Device or resource busy

`rm` gets `EBUSY` in three common cases:

- **The path is a mount point.** Unmount it first (`findmnt /path` tells you whether it is one).
- **Inside a Docker container,** files like `/etc/hosts` and `/etc/resolv.conf` are bind-mounted from the host. You can edit them but not delete or replace them. Change them with Docker's options (`--add-host`, `--dns`) instead.
- **A file on a network or FUSE filesystem** that another client or process still has open.

## Text file busy

`Text file busy` (`ETXTBSY`) is a different error with a similar name. "Text" is the old Unix word for a program's code. You get it when you try to write to a program file while it's running:

```
cp: cannot create regular file './server': Text file busy
```

Stop the running program first, or install the new version under a new name and move it into place (`mv`), which replaces the file instead of writing into it. You can also get it the other way round, running a script that's still open for writing, for example right after creating it in the same step; close it first.

## On a Mac: "The disk wasn't ejected"

macOS says it in Finder: *The disk wasn't ejected because one or more programs may be using it.* Find them the same way:

```bash
sudo lsof /Volumes/USB
```

`diskutil unmount /Volumes/USB` in Terminal also names the process that refused, on recent macOS. Spotlight indexing (`mds`) and backup tools are frequent holders; they let go after a minute, or you can force it with `diskutil unmount force /Volumes/USB`.

## On Windows: "The file is open in another program"

Windows says *The action can't be completed because the file is open in another program.* To find the program:

- **Resource Monitor:** run `resmon`, open the **CPU** tab, and type the file name in **Associated Handles**.
- **Handle**, from Microsoft's Sysinternals: `handle.exe report.xlsx`.
- **File Locksmith** in Microsoft PowerToys: right-click the file and open it with File Locksmith, which lists the programs using it and can end them.

## With proc

[proc](https://github.com/yazeed/proc-cli) answers two of these questions directly:

```bash
# what has this file open?
proc for /mnt/usb/notes.txt

# who's working in or below it?
proc in /mnt/usb
```

`proc for` checks one path: a process that has it open, runs it, maps it, or has it as its working directory. `proc in` lists the processes whose working directory is in that directory or any folder below it, which catches the shells that keep a mount busy. For every open file anywhere on a mount, `lsof <mountpoint>` above is still the tool. On Linux, `proc for` reads `/proc` itself and needs nothing installed; on macOS it asks `lsof`.

## Install

```bash
# macOS
brew install yazeed/proc/proc

# npm or bun, any OS
npm install -g proc-cli

# Rust
cargo install proc-cli
```

The [install page](/install) lists every other way.
