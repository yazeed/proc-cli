//! `proc on` - Port/process lookup
//!
//! Usage:
//!   proc on :3000              # What process is on port 3000?
//!   proc on :3000 :8080        # What's on several ports? (separate arguments)
//!   proc on 1234               # What ports is PID 1234 listening on?
//!   proc on node               # What ports are the processes named node listening on?
//!   proc on node --cmd         # ... or with node anywhere in their command line
//!   proc on node --in .        # Node processes in cwd and their ports
//!
//! Commas still work (`proc on :3000,:8080`) but are deprecated.

use crate::core::{
    find_ports_for_pid, flatten_targets, parse_target, resolve_in_dir, resolve_target,
    ruled_out_by, with_ports, Guards, NameMatch, PortInfo, Process, TargetType,
};
use crate::error::{ExitCode, ProcError, Result};
use crate::out_println;
use crate::ui::{format_duration, format_memory, out, Printer};
use clap::Args;
use colored::*;
use serde::Serialize;
use std::io::Write;

/// Show what's on a port, or what ports a process is on
#[derive(Args, Debug, Clone)]
pub struct OnCommand {
    /// Target(s): :port, PID, or process name (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    pub target: Vec<String>,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Match name targets and --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Show verbose output (full command line)
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// `--in` as `execute` resolved it, absolute; every check uses this one
    #[arg(skip)]
    in_dir_path: Option<std::path::PathBuf>,
}

impl OnCommand {
    /// Executes the on command, performing bidirectional port/process lookup.
    pub fn execute(&self) -> Result<()> {
        // Resolve --in once: a relative directory needs the current one, which
        // may be gone (an error here), and every check after this one uses the
        // absolute path. A missing directory with nothing running there is a
        // typo, as on every command, not targets that are not found in it.
        let in_dir_path = resolve_in_dir(&self.in_dir)?;
        crate::core::require_in_dir_live(&self.in_dir, &in_dir_path)?;
        let resolved = OnCommand {
            in_dir_path,
            ..self.clone()
        };
        resolved.run()
    }

    /// `execute` once `--in` is resolved
    fn run(&self) -> Result<()> {
        let targets = flatten_targets(&self.target);

        // For single target, use original behavior
        if targets.len() == 1 {
            return match parse_target(&targets[0]) {
                TargetType::Port(port) => self.show_process_on_port(port),
                TargetType::Pid(pid) => self.show_ports_for_pid(pid),
                TargetType::Name(name) => self.show_ports_for_name(&name),
            };
        }

        if self.json {
            return self.show_multi_json(&targets);
        }

        // Multi-target handling
        let mut not_found = Vec::new();

        for target in &targets {
            match parse_target(target) {
                TargetType::Port(port) => {
                    if let Err(e) = self.show_process_on_port(port) {
                        out_println!("{} Port {}: {}", "⚠".yellow(), port, e);
                        not_found.push(target.clone());
                    }
                }
                TargetType::Pid(pid) => {
                    if let Err(e) = self.show_ports_for_pid(pid) {
                        out_println!("{} PID {}: {}", "⚠".yellow(), pid, e);
                        not_found.push(target.clone());
                    }
                }
                TargetType::Name(ref name) => {
                    if let Err(e) = self.show_ports_for_name(name) {
                        out_println!("{} '{}': {}", "⚠".yellow(), name, e);
                        not_found.push(target.clone());
                    }
                }
            }
        }

        if not_found.len() == targets.len() {
            // Each target already printed its own warning
            return Err(ProcError::reported(
                ExitCode::NotFound,
                format!("No process found matching '{}'", self.target.join(" ")),
            ));
        }

        Ok(())
    }

    /// Multi-target JSON: one envelope with a result per matched process
    fn show_multi_json(&self, targets: &[String]) -> Result<()> {
        let mut results: Vec<TargetResult> = Vec::new();
        let mut not_found: Vec<String> = Vec::new();

        for target in targets {
            let found = match parse_target(target) {
                TargetType::Port(port) => self.lookup_port(port).map(port_hits),
                TargetType::Pid(pid) => self.lookup_pid(pid).map(|found| vec![found]),
                TargetType::Name(ref name) => self.lookup_name(name),
            };

            match found {
                Ok(found) if !found.is_empty() => {
                    results.extend(found.into_iter().map(|(process, ports)| TargetResult {
                        target: target.clone(),
                        process,
                        ports,
                    }))
                }
                _ => not_found.push(target.clone()),
            }
        }

        if results.is_empty() {
            return Err(ProcError::ProcessNotFound(self.target.join(" ")));
        }

        Printer::from_flags(true, self.verbose).print_json(&MultiTargetOutput {
            action: "on",
            success: true,
            count: results.len(),
            results,
            not_found,
        });

        Ok(())
    }

    /// Check if process matches --in filter
    fn matches_in_filter(&self, proc: &Process) -> bool {
        self.in_dir_path
            .as_ref()
            .is_none_or(|dir_path| crate::core::filters::matches_in_filter(proc, dir_path))
    }

    /// Check if process matches --by filter
    fn matches_by_filter(&self, proc: &Process) -> bool {
        if let Some(ref name) = self.by_name {
            crate::core::matches_by_filter(proc, name, self.mode())
        } else {
            true
        }
    }

    /// How names and `--by` match
    fn mode(&self) -> NameMatch {
        NameMatch::from_flag(self.cmd)
    }

    /// `--in` and `--by`, for naming the ones that rule a target out
    fn guards(&self) -> Guards {
        Guards::new(&self.in_dir, &self.by_name, None)
    }

    /// Check if process matches all filters (--in and --by)
    fn matches_filters(&self, proc: &Process) -> bool {
        self.matches_in_filter(proc) && self.matches_by_filter(proc)
    }

    /// Find every listener on a port with its process, applying --in/--by
    fn lookup_port(&self, port: u16) -> Result<Vec<(PortInfo, Option<Process>)>> {
        let listeners = PortInfo::holders_of(port)?;
        if listeners.is_empty() {
            return Err(ProcError::PortNotFound(port));
        }

        let mut holders = Vec::new();
        for listener in listeners {
            let process = Process::find_by_pid(listener.pid)?;
            holders.push((listener, process));
        }
        self.filter_holders(port, holders)
    }

    /// Apply --in/--by to the holders of a port and take the CPU reading of
    /// the ones that stay, all in one wait. A holder whose process is `None`
    /// ended after its port was listed, and is kept without one. When the
    /// filters leave no holder the port counts as not found.
    fn filter_holders(
        &self,
        port: u16,
        holders: Vec<(PortInfo, Option<Process>)>,
    ) -> Result<Vec<(PortInfo, Option<Process>)>> {
        let found: Vec<Process> = holders.iter().filter_map(|(_, p)| p.clone()).collect();
        let mut kept: Vec<(PortInfo, Option<Process>)> = holders
            .into_iter()
            .filter(|(_, process)| process.as_ref().is_none_or(|p| self.matches_filters(p)))
            .collect();
        if kept.is_empty() {
            return Err(ProcError::FilteredOut {
                target: format!(":{port}"),
                ruled_out_by: ruled_out_by(&found, &self.guards(), self.mode())?,
            });
        }

        // Real CPU reading for what's shown
        let mut live: Vec<Process> = kept.iter().filter_map(|(_, p)| p.clone()).collect();
        Process::measure_cpu(&mut live);
        let mut live = live.into_iter();
        for (_, slot) in kept.iter_mut() {
            if slot.is_some() {
                *slot = live.next();
            }
        }
        Ok(kept)
    }

    /// Show what process is on a specific port
    fn show_process_on_port(&self, port: u16) -> Result<()> {
        let holders = self.lookup_port(port)?;

        if self.json {
            let printer = Printer::from_flags(true, self.verbose);
            let (first, _) = &holders[0];
            let processes: Vec<&Process> = holders.iter().filter_map(|(_, p)| p.as_ref()).collect();
            let output = PortLookupOutput {
                action: "on",
                query_type: "port_to_process",
                success: true,
                port: Some(first.port),
                protocol: Some(format!("{:?}", first.protocol).to_lowercase()),
                address: first.address.clone(),
                // the first holder, as before `processes` existed
                process: processes.first().copied(),
                processes: Some(processes),
                ports: None,
            };
            printer.print_json(&output);
        } else {
            // A failed write is recorded by `out`, which decides the exit code
            out::recorded(self.print_process_on_port(&mut out::stdout(), port, &holders));
        }

        Ok(())
    }

    /// Find a process by PID and its listening ports, applying --in/--by
    fn lookup_pid(&self, pid: u32) -> Result<(Process, Vec<PortInfo>)> {
        let mut process = Process::find_by_pid(pid)?
            .ok_or_else(|| ProcError::ProcessNotFound(pid.to_string()))?;
        // Real CPU reading for what's shown
        Process::measure_cpu(std::slice::from_mut(&mut process));

        // Apply --in and --by filters if present
        if !self.matches_filters(&process) {
            return Err(ProcError::FilteredOut {
                target: pid.to_string(),
                ruled_out_by: ruled_out_by(&[process], &self.guards(), self.mode())?,
            });
        }

        let ports = find_ports_for_pid(pid)?;
        Ok((process, ports))
    }

    /// Show what ports a PID is listening on
    fn show_ports_for_pid(&self, pid: u32) -> Result<()> {
        let (process, ports) = self.lookup_pid(pid)?;

        if self.json {
            let printer = Printer::from_flags(true, self.verbose);
            let output = PortLookupOutput {
                action: "on",
                query_type: "process_to_ports",
                success: true,
                port: None,
                protocol: None,
                address: None,
                process: Some(&process),
                processes: None,
                ports: Some(&ports),
            };
            printer.print_json(&output);
        } else {
            out::recorded(self.print_ports_for_process(&mut out::stdout(), &process, &ports));
        }

        Ok(())
    }

    /// Find processes by name and their listening ports, applying --in/--by
    fn lookup_name(&self, name: &str) -> Result<Vec<(Process, Vec<PortInfo>)>> {
        // A name that matches nothing is an error here, never an empty list
        let mut processes = resolve_target(name, NameMatch::from_flag(self.cmd))?;

        // Apply --in and --by filters if present
        if self.in_dir.is_some() || self.by_name.is_some() {
            let found = processes.clone();
            processes.retain(|p| self.matches_filters(p));
            if processes.is_empty() {
                return Err(ProcError::FilteredOut {
                    target: name.to_string(),
                    ruled_out_by: ruled_out_by(&found, &self.guards(), self.mode())?,
                });
            }
        }

        // Real CPU readings for what's shown, one wait for all of them
        Process::measure_cpu(&mut processes);

        with_ports(processes)
    }

    /// Show what ports processes with a given name are listening on
    fn show_ports_for_name(&self, name: &str) -> Result<()> {
        let all_results = self.lookup_name(name)?;

        if self.json {
            let printer = Printer::from_flags(true, self.verbose);
            let results: Vec<_> = all_results
                .iter()
                .map(|(proc, ports)| ProcessPortsJson {
                    process: proc,
                    ports,
                })
                .collect();
            let output = MultiProcessPortsOutput {
                action: "on",
                success: true,
                count: results.len(),
                results: &results,
            };
            printer.print_json(&output);
        } else {
            for (proc, ports) in &all_results {
                out::recorded(self.print_ports_for_process(&mut out::stdout(), proc, ports));
            }
        }

        Ok(())
    }

    /// What is on `port`: the line saying so, then each holder
    fn print_process_on_port(
        &self,
        out: &mut impl Write,
        port: u16,
        holders: &[(PortInfo, Option<Process>)],
    ) -> std::io::Result<()> {
        let who = if holders.len() > 1 {
            format!(" {} processes", holders.len())
        } else {
            String::new()
        };
        writeln!(
            out,
            "{} Port {} is used by{}:",
            "✓".green().bold(),
            port.to_string().cyan().bold(),
            who
        )?;
        writeln!(out)?;

        for (port_info, process) in holders {
            self.print_holder(out, port_info, process.as_ref())?;
        }
        Ok(())
    }

    fn print_holder(
        &self,
        out: &mut impl Write,
        port_info: &PortInfo,
        process: Option<&Process>,
    ) -> std::io::Result<()> {
        writeln!(
            out,
            "  {} {} (PID {})",
            "Process:".bright_black(),
            port_info.process_name.white().bold(),
            port_info.pid.to_string().cyan()
        )?;

        if let Some(proc) = process {
            if let Some(ref cwd) = proc.cwd {
                writeln!(out, "  {} {}", "Directory:".bright_black(), cwd)?;
            }
            if let Some(ref path) = proc.exe_path {
                writeln!(out, "  {} {}", "Path:".bright_black(), path.bright_black())?;
            }
        }

        let addr = port_info.address.as_deref().unwrap_or("*");
        writeln!(
            out,
            "  {} {} on {}",
            "Listening:".bright_black(),
            format!("{:?}", port_info.protocol).to_uppercase(),
            addr
        )?;

        if let Some(proc) = process {
            writeln!(
                out,
                "  {} {:.1}% CPU, {}",
                "Resources:".bright_black(),
                proc.cpu_percent,
                format_memory(proc.memory_mb)
            )?;

            if let Some(uptime) = proc.uptime_secs {
                writeln!(
                    out,
                    "  {} {}",
                    "Uptime:".bright_black(),
                    format_duration(uptime)
                )?;
            }

            if self.verbose {
                if let Some(ref cmd) = proc.command {
                    writeln!(
                        out,
                        "  {} {}",
                        "Command:".bright_black(),
                        cmd.bright_black()
                    )?;
                }
            }
        }

        writeln!(out)
    }

    fn print_ports_for_process(
        &self,
        out: &mut impl Write,
        process: &Process,
        ports: &[PortInfo],
    ) -> std::io::Result<()> {
        writeln!(
            out,
            "{} {} (PID {}) is listening on:",
            "✓".green().bold(),
            process.name.white().bold(),
            process.pid.to_string().cyan().bold()
        )?;
        writeln!(out)?;

        if ports.is_empty() {
            writeln!(out, "  {} No listening ports", "ℹ".blue())?;
        } else {
            for port_info in ports {
                let addr = port_info.address.as_deref().unwrap_or("*");
                writeln!(
                    out,
                    "  {} :{} ({} on {})",
                    "→".bright_black(),
                    port_info.port.to_string().cyan(),
                    format!("{:?}", port_info.protocol).to_uppercase(),
                    addr
                )?;
            }
        }

        if process.cwd.is_some() || process.uptime_secs.is_some() {
            writeln!(out)?;
        }
        if let Some(ref cwd) = process.cwd {
            writeln!(out, "  {} {}", "Directory:".bright_black(), cwd)?;
        }
        if let Some(uptime) = process.uptime_secs {
            writeln!(
                out,
                "  {} {}",
                "Uptime:".bright_black(),
                format_duration(uptime)
            )?;
        }

        if self.verbose {
            if let Some(ref path) = process.exe_path {
                writeln!(out, "  {} {}", "Path:".bright_black(), path.bright_black())?;
            }
            if let Some(ref cmd) = process.command {
                writeln!(
                    out,
                    "  {} {}",
                    "Command:".bright_black(),
                    cmd.bright_black()
                )?;
            }
        }

        writeln!(out)
    }
}

/// What a port lookup contributes to the multi-target JSON: each holder's
/// process with the port it holds. A holder whose process ended after the port
/// was listed adds nothing.
fn port_hits(holders: Vec<(PortInfo, Option<Process>)>) -> Vec<(Process, Vec<PortInfo>)> {
    holders
        .into_iter()
        .filter_map(|(port_info, process)| process.map(|p| (p, vec![port_info])))
        .collect()
}

#[derive(Serialize)]
struct PortLookupOutput<'a> {
    action: &'static str,
    query_type: &'static str,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    process: Option<&'a Process>,
    /// Every process holding the port (`process` is the first of them)
    #[serde(skip_serializing_if = "Option::is_none")]
    processes: Option<Vec<&'a Process>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ports: Option<&'a [PortInfo]>,
}

#[derive(Serialize)]
struct ProcessPortsJson<'a> {
    process: &'a Process,
    ports: &'a [PortInfo],
}

#[derive(Serialize)]
struct TargetResult {
    target: String,
    process: Process,
    ports: Vec<PortInfo>,
}

#[derive(Serialize)]
struct MultiTargetOutput {
    action: &'static str,
    success: bool,
    count: usize,
    results: Vec<TargetResult>,
    not_found: Vec<String>,
}

#[derive(Serialize)]
struct MultiProcessPortsOutput<'a> {
    action: &'static str,
    success: bool,
    count: usize,
    results: &'a Vec<ProcessPortsJson<'a>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ProcessStatus, Protocol};
    use crate::test_support::assert_stops_at_a_failed_write;

    fn command(in_dir: Option<&str>) -> OnCommand {
        let in_dir = in_dir.map(String::from);
        OnCommand {
            target: vec!["x".to_string()],
            // as `execute` resolves it
            in_dir_path: resolve_in_dir(&in_dir).unwrap(),
            in_dir,
            by_name: None,
            cmd: false,
            json: false,
            verbose: false,
        }
    }

    /// A process record with no working directory, which some processes really
    /// are (a zombie has none on Linux)
    fn without_cwd() -> Process {
        Process {
            pid: 1,
            name: "x".to_string(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Sleeping,
            user: None,
            parent_pid: None,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
        }
    }

    fn listener() -> PortInfo {
        PortInfo {
            port: 4000,
            protocol: Protocol::Tcp,
            pid: 1,
            process_name: "x".to_string(),
            address: None,
        }
    }

    #[test]
    fn a_process_with_no_working_directory_fails_in() {
        assert!(!command(Some("/")).matches_in_filter(&without_cwd()));
        assert!(command(None).matches_in_filter(&without_cwd()));
    }

    #[test]
    fn a_port_whose_process_ended_has_no_process_to_filter() {
        let holders = command(Some("/"))
            .filter_holders(4000, vec![(listener(), None)])
            .unwrap();
        assert_eq!(holders.len(), 1);
        assert_eq!(holders[0].0.port, 4000);
        assert!(holders[0].1.is_none());
    }

    #[test]
    fn the_filters_drop_the_holders_they_do_not_match() {
        let mut elsewhere = without_cwd();
        elsewhere.pid = 2;
        elsewhere.cwd = Some("/definitely/not/here".to_string());
        let mut here = without_cwd();
        here.pid = 3;
        here.cwd = Some(
            std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );

        let holders = vec![
            (listener(), Some(elsewhere)),
            (listener(), Some(here.clone())),
            (listener(), None),
        ];
        let kept = command(Some("."))
            .filter_holders(4000, holders.clone())
            .unwrap();
        let pids: Vec<_> = kept
            .iter()
            .map(|(_, p)| p.as_ref().map(|p| p.pid))
            .collect();
        assert_eq!(pids, vec![Some(3), None]);

        // nothing left: the port counts as not found
        let none = command(Some("/definitely/not/here/either"))
            .filter_holders(4000, holders[..1].to_vec());
        assert!(
            matches!(&none, Err(ProcError::FilteredOut { target, .. }) if target == ":4000"),
            "{none:?}"
        );
    }

    #[test]
    fn a_port_whose_process_ended_adds_nothing_to_the_json_results() {
        assert!(port_hits(vec![(listener(), None)]).is_empty());
        let hits = port_hits(vec![
            (listener(), Some(without_cwd())),
            (listener(), None),
            (listener(), Some(without_cwd())),
        ]);
        assert_eq!(hits.len(), 2);
    }

    /// What a print function writes, with colours off
    fn printed(print: impl FnOnce(&mut Vec<u8>) -> std::io::Result<()>) -> String {
        colored::control::set_override(false);
        let mut out = Vec::new();
        print(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_port_without_a_process_still_prints() {
        // Neither the process nor its details: the address shows as `*`
        let mut cmd = command(None);
        cmd.verbose = true;
        let out = printed(|w| cmd.print_process_on_port(w, 4000, &[(listener(), None)]));
        assert_eq!(
            out,
            "✓ Port 4000 is used by:\n\n  Process: x (PID 1)\n  Listening: TCP on *\n\n"
        );
    }

    #[test]
    fn a_port_with_several_holders_says_so_and_lists_each() {
        let mut second = listener();
        second.pid = 2;
        second.address = Some("::1".to_string());
        let out = printed(|w| {
            command(None).print_process_on_port(w, 4000, &[(listener(), None), (second, None)])
        });
        assert_eq!(
            out,
            "✓ Port 4000 is used by 2 processes:\n\n\
             \x20 Process: x (PID 1)\n  Listening: TCP on *\n\n\
             \x20 Process: x (PID 2)\n  Listening: TCP on ::1\n\n"
        );
    }

    #[test]
    fn a_process_missing_its_details_prints_without_them() {
        let mut cmd = command(None);
        cmd.verbose = true;

        let out =
            printed(|w| cmd.print_process_on_port(w, 4000, &[(listener(), Some(without_cwd()))]));
        assert!(out.contains("Listening: TCP on *\n"), "{out}");
        assert!(out.contains("Resources: 0.0% CPU"), "{out}");
        for label in ["Directory:", "Path:", "Uptime:", "Command:"] {
            assert!(!out.contains(label), "{label} printed for nothing: {out}");
        }

        let out = printed(|w| cmd.print_ports_for_process(w, &without_cwd(), &[listener()]));
        assert_eq!(
            out,
            "✓ x (PID 1) is listening on:\n\n  → :4000 (TCP on *)\n\n"
        );
    }

    /// A process with every detail the printers can show
    fn detailed() -> Process {
        let mut p = without_cwd();
        p.cwd = Some("/work/app".to_string());
        p.exe_path = Some("/usr/bin/x".to_string());
        p.command = Some("x --serve".to_string());
        p.uptime_secs = Some(90);
        p
    }

    #[test]
    fn a_failed_write_ends_the_port_lookup() {
        let mut cmd = command(None);
        cmd.verbose = true;
        let p = detailed();
        let held = [(listener(), Some(p))];
        assert_stops_at_a_failed_write(|w| cmd.print_process_on_port(w, 4000, &held));
        assert_stops_at_a_failed_write(|w| {
            cmd.print_process_on_port(w, 4000, &[(listener(), None)])
        });
    }

    #[test]
    fn a_failed_write_ends_the_process_lookup() {
        let mut cmd = command(None);
        cmd.verbose = true;
        let p = detailed();
        assert_stops_at_a_failed_write(|w| cmd.print_ports_for_process(w, &p, &[listener()]));
        assert_stops_at_a_failed_write(|w| cmd.print_ports_for_process(w, &p, &[]));
    }
}
