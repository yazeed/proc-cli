//! The command-line definition: `Cli` and its subcommands.
//!
//! Lives in the library so tests can read the clap definitions
//! (`Cli::command()`) and check them against what they exercise.

use crate::commands::{
    ByCommand, ForCommand, FreeCommand, FreezeCommand, InCommand, InfoCommand, KillCommand,
    ListCommand, OnCommand, OrphansCommand, PortsCommand, StopCommand, StuckCommand, ThawCommand,
    TreeCommand, UnstickCommand, WaitCommand, WatchCommand, WhyCommand,
};
use clap::builder::styling::Styles;
use clap::builder::StyledStr;
use clap::{Parser, Subcommand};
use clap_complete::Shell;

const VERSION_INFO: &str = concat!(
    env!("PROC_VERSION"),
    "\nhttps://github.com/yazeed/proc-cli",
    "\nLicense: MIT"
);

/// The look of the help. This is clap's own default, named here so the help
/// footers below are drawn from the same styles as the `Usage:` and `Options:`
/// headers around them.
pub const STYLES: Styles = Styles::styled();

/// The examples that open the long help, in groups; the footer follows them
const EXAMPLES: &[(&str, &[(&str, &str)])] = &[
    (
        "Port/Process Lookup",
        &[
            ("proc on :3000", "What's on port 3000?"),
            ("proc on :3000 :8080", "What's on several ports?"),
            (
                "proc on 1234",
                "What is PID 1234 and what ports is it using?",
            ),
            (
                "proc on node",
                "What ports are the processes named node using?",
            ),
        ],
    ),
    (
        "Find by File",
        &[
            (
                "proc for ./server",
                "Processes running this program (or holding it open)",
            ),
            (
                "proc for /usr/bin/node",
                "Processes running this executable",
            ),
            (
                "proc for app.log",
                "What has this file open? (not searched on Windows)",
            ),
        ],
    ),
    (
        "Filter by Name",
        &[
            ("proc by node", "Processes named 'node' (the whole name)"),
            (
                "proc by node --cmd",
                "Anything with 'node' in its name or command line",
            ),
            ("proc by node --in .", "Node processes in current directory"),
            ("proc by node --min-cpu 5", "Node processes using >5% CPU"),
        ],
    ),
    (
        "Filter by Directory",
        &[
            ("proc in .", "Processes in current directory"),
            ("proc in . --by node", "Node processes in cwd"),
        ],
    ),
    (
        "List All",
        &[
            ("proc list", "All processes"),
            ("proc list --min-cpu 10", "Processes using >10% CPU"),
        ],
    ),
    (
        "Info/Kill/Stop (multi-target)",
        &[
            ("proc info :3000 :8080", "Info for several targets"),
            (
                "proc kill :3000 node",
                "Kill port 3000 and the processes named node",
            ),
            (
                "proc kill serve --cmd",
                "Kill processes with 'serve' in their command line",
            ),
            (
                "proc kill 1234 5678 :8080",
                "Kill two PIDs and whatever is on 8080",
            ),
            ("proc stop :3000 :8080", "Stop several targets gracefully"),
        ],
    ),
    (
        "Watch (Real-Time)",
        &[
            ("proc watch", "Watch all processes (alias: proc top)"),
            ("proc watch node", "Watch the processes named node"),
            ("proc watch :3000", "Watch process on port 3000"),
            ("proc watch :3000 :8080 node", "Watch several targets"),
            ("proc watch -n 1 --sort mem", "1s refresh, sorted by memory"),
            (
                "proc watch --in . --by node",
                "Node processes in current directory",
            ),
        ],
    ),
    (
        "Freeze/Thaw",
        &[
            (
                "proc freeze node",
                "Pause the processes named node (SIGSTOP)",
            ),
            ("proc freeze :3000 --yes", "Freeze by port, skip confirm"),
            ("proc thaw node", "Resume frozen processes (SIGCONT)"),
        ],
    ),
    (
        "Free Ports",
        &[
            ("proc free :3000", "Stop what holds it, check it's free"),
            ("proc free :3000 :8080 --yes", "Free several ports"),
        ],
    ),
    (
        "Diagnostics",
        &[
            ("proc why :3000", "Why is port 3000 busy? (ancestry)"),
            ("proc orphans --in .", "Leftovers from this project"),
            ("proc orphans --in . --kill", "End them, after asking"),
        ],
    ),
    (
        "Wait",
        &[
            ("proc wait node", "Wait until node processes exit"),
            ("proc wait 1234 5678", "Wait until both exit"),
            (
                "proc wait :3000 --timeout 60",
                "Wait up to 60s for port to free",
            ),
            ("proc wait node -n 10 -q", "Check every 10s, quiet mode"),
        ],
    ),
    (
        "Other",
        &[
            ("proc ports", "List all listening TCP ports"),
            ("proc tree --min-cpu 5", "Process tree filtered by CPU"),
            ("proc stuck", "Find hung processes"),
            (
                "proc unstick --force",
                "Recover or terminate stuck processes",
            ),
            (
                "proc stop nginx --signal USR1",
                "Send a different first signal (force-killed after --timeout if it lives)",
            ),
        ],
    ),
];

/// One labelled block of the footer: a header, then rows of a literal (what to
/// type) and what it means. A row with an empty literal continues the line above.
struct Section {
    header: &'static str,
    rows: &'static [(&'static str, &'static str)],
}

const TARGETS: Section = Section {
    header: "Targets:",
    rows: &[
        (":3000", "A port"),
        ("1234", "A PID"),
        ("node", "A name"),
        (":3000 node", "Several: separate arguments"),
    ],
};

const NAMES: Section = Section {
    header: "Names:",
    rows: &[
        (
            "node",
            "The process name, the file name of its executable or of argv[0],",
        ),
        (
            "",
            "ignoring case (like pkill -x); 'node' is not 'Claude Helper'",
        ),
        (
            "--cmd",
            "Any part of the name or the full command line (like pkill -f)",
        ),
        ("./server", "Not a name: use --cmd, or proc for <file>"),
        (
            "node*",
            "Not expanded by proc, but by your shell if unquoted: use --cmd",
        ),
    ],
};

const NOTES: Section = Section {
    header: "Notes:",
    rows: &[
        (
            ":3000,:8080",
            "Deprecated, to be removed; prints a note on stderr",
        ),
        ("", "(not with --json)"),
        (
            "--in DIR",
            "Takes the next word as DIR unless it starts with '-':",
        ),
        ("", "put it after the targets, or write --in=DIR"),
    ],
};

const REPOSITORY: &str = "https://github.com/yazeed/proc-cli";

/// Sections in the look of clap's `Options:` list: the header in the header
/// style, each literal in the literal style, the meanings aligned in one column.
fn render_sections(sections: &[&Section]) -> String {
    let literal_width = sections
        .iter()
        .flat_map(|section| section.rows.iter())
        .map(|(literal, _)| literal.chars().count())
        .max()
        .unwrap_or(0);
    let (header, literal) = (STYLES.get_header(), STYLES.get_literal());
    let mut out = String::new();
    for section in sections {
        out.push_str(&format!("{header}{}{header:#}\n", section.header));
        for (text, meaning) in section.rows {
            let pad = " ".repeat(literal_width - text.chars().count());
            if text.is_empty() {
                out.push_str(&format!("  {pad}  {meaning}\n"));
            } else {
                out.push_str(&format!("  {literal}{text}{literal:#}{pad}  {meaning}\n"));
            }
        }
        out.push('\n');
    }
    out
}

/// The examples in the footer's look: one `Examples:` header, each group's
/// label, then the commands in the literal style with their meanings aligned
fn render_examples() -> String {
    let width = EXAMPLES
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|(command, _)| command.chars().count())
        .max()
        .unwrap_or(0);
    let (header, literal) = (STYLES.get_header(), STYLES.get_literal());
    let mut out = format!("{header}Examples:{header:#}\n");
    for (label, rows) in EXAMPLES {
        out.push_str(&format!("\n  {label}\n"));
        for (command, meaning) in *rows {
            let pad = " ".repeat(width - command.chars().count());
            out.push_str(&format!(
                "    {literal}{command}{literal:#}{pad}  {meaning}\n"
            ));
        }
    }
    out
}

/// The closing line of both helps
fn more(what: &str) -> String {
    let header = STYLES.get_header();
    format!("{header}More:{header:#} {what}")
}

/// The footer of `proc -h`
fn short_footer() -> StyledStr {
    let mut out = String::from("\n");
    out.push_str(&render_sections(&[&TARGETS]));
    out.push_str(&more(&format!("proc --help for examples, or {REPOSITORY}")));
    StyledStr::from(out)
}

/// The long help's closing text: the examples, then the footer
fn long_footer() -> StyledStr {
    let mut out = render_examples();
    out.push('\n');
    out.push_str(&render_sections(&[&TARGETS, &NAMES, &NOTES]));
    out.push_str(&more(REPOSITORY));
    StyledStr::from(out)
}

/// Each command's own examples, at the end of its `--help` and its man page
const COMMAND_EXAMPLES: &[(&str, &[(&str, &str)])] = &[
    (
        "on",
        &[
            ("proc on :3000", "What's on port 3000?"),
            ("proc on node", "Which ports the processes named node use"),
        ],
    ),
    (
        "why",
        &[
            ("proc why :3000", "What started what holds port 3000"),
            ("proc why 1234", "Where PID 1234 came from"),
        ],
    ),
    (
        "for",
        &[
            ("proc for ./server", "What runs this program"),
            ("proc for app.log", "What has this file open"),
        ],
    ),
    (
        "by",
        &[
            ("proc by node", "The processes named node"),
            (
                "proc by node --cmd",
                "... or with node in their command line",
            ),
        ],
    ),
    (
        "in",
        &[
            ("proc in .", "Everything running in this project"),
            ("proc in . --by node", "Its node processes"),
        ],
    ),
    (
        "list",
        &[
            (
                "proc list --sort cpu --limit 10",
                "The 10 busiest processes",
            ),
            ("proc list node python", "The ones named node or python"),
        ],
    ),
    (
        "info",
        &[
            ("proc info 1234", "Everything about PID 1234"),
            ("proc info :3000 node", "About each of several targets"),
        ],
    ),
    (
        "ports",
        &[
            ("proc ports", "Every listening port"),
            ("proc ports --exposed", "Only the ones open to the network"),
        ],
    ),
    (
        "tree",
        &[
            ("proc tree node", "The tree under each process named node"),
            (
                "proc tree :3000 --ancestors",
                "The parents above the holder of port 3000",
            ),
        ],
    ),
    (
        "watch",
        &[
            (
                "proc watch node -n 1",
                "The node processes, refreshed every second",
            ),
            ("proc watch --json | head -n1", "One frame, for a script"),
        ],
    ),
    (
        "stuck",
        &[
            ("proc stuck", "What looks hung"),
            ("proc stuck --kill --dry-run", "What --kill would end"),
        ],
    ),
    (
        "unstick",
        &[
            (
                "proc unstick --dry-run",
                "What it would try, on which processes",
            ),
            ("proc unstick 1234 --force", "Recover PID 1234, or end it"),
        ],
    ),
    (
        "freeze",
        &[
            ("proc freeze node --dry-run", "What it would pause"),
            (
                "proc freeze :3000",
                "Pause what's on port 3000, after asking",
            ),
        ],
    ),
    (
        "thaw",
        &[
            ("proc thaw node", "Resume the node processes, after asking"),
            (
                "proc thaw :3000 --yes",
                "Resume a port's holder you've seen",
            ),
        ],
    ),
    (
        "orphans",
        &[
            ("proc orphans --in .", "Leftovers from this project"),
            ("proc orphans --in . --kill", "End them, after asking"),
        ],
    ),
    (
        "kill",
        &[
            ("proc kill :3000", "Everything on port 3000, after asking"),
            (
                "proc kill node --in . --dry-run",
                "What it would kill in this project",
            ),
        ],
    ),
    (
        "stop",
        &[
            (
                "proc stop :3000",
                "Ask what's on port 3000 to exit, after asking",
            ),
            (
                "proc stop worker --signal INT",
                "Start with SIGINT instead of SIGTERM",
            ),
        ],
    ),
    (
        "free",
        &[
            (
                "proc free :3000",
                "Stop what holds port 3000, check it's free",
            ),
            ("proc free :3000 --dry-run", "What it would stop"),
        ],
    ),
    (
        "wait",
        &[
            (
                "proc wait 1234 --timeout 60",
                "Until PID 1234 exits, or a minute",
            ),
            ("proc wait :3000", "Until what holds port 3000 exits"),
        ],
    ),
];

/// The exit codes of a command that changes processes and takes targets
const TARGETED_EXITS: &[(&str, &str)] = &[
    ("0", "Done, or the dry run found what it would change"),
    (
        "1",
        "A process could not be changed (the system refused it)",
    ),
    (
        "2",
        "Nothing matched, or a guard (--by, --in, --start-time) ruled it out",
    ),
    (
        "3",
        "Permission denied, or a port another user's process holds",
    ),
    (
        "4",
        "Nothing was done: a wrong argument, no --yes where nobody can be",
    ),
    (
        "",
        "asked (--json, no terminal), or not supported on this OS",
    ),
];

/// `free`'s exit codes: a port nobody holds is already free
const FREE_EXITS: &[(&str, &str)] = &[
    (
        "0",
        "Every port is free (or already was), or the dry run listed its holders",
    ),
    ("1", "A port stayed in use"),
    ("3", "A port another user's process holds"),
    (
        "4",
        "Nothing was done: a target that isn't a port, a wrong argument,",
    ),
    (
        "",
        "or no --yes where nobody can be asked (--json, no terminal)",
    ),
];

/// The exit codes of `stuck` and `orphans`, which change processes only with --kill
const SEARCH_EXITS: &[(&str, &str)] = &[
    (
        "0",
        "Done, found nothing, or the dry run listed what --kill would end",
    ),
    ("1", "A process could not be killed (the system refused it)"),
    (
        "4",
        "Nothing was done: a wrong argument, or no --yes where nobody can be",
    ),
    (
        "",
        "asked (--json, no terminal); orphans exits 4 on Windows",
    ),
];

/// `wait`'s exit codes: whether the processes exited is its answer
const WAIT_EXITS: &[(&str, &str)] = &[
    ("0", "Every process exited"),
    ("1", "--timeout ran out first"),
    (
        "2",
        "Nothing matched, or a guard (--by, --in, --start-time) ruled it out",
    ),
    ("3", "A port another user's process holds: it is in use"),
    ("4", "A wrong argument"),
];

/// The end of a command's `--help` and man page: its examples, its exit codes
/// where they say whether it did what was asked, and its page on the website
fn command_footer(name: &str) -> StyledStr {
    let (header, literal) = (STYLES.get_header(), STYLES.get_literal());
    let examples = COMMAND_EXAMPLES
        .iter()
        .find(|(command, _)| *command == name)
        .map(|(_, rows)| *rows)
        .unwrap_or_default();
    let width = examples
        .iter()
        .map(|(example, _)| example.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = format!("{header}Examples:{header:#}\n");
    for (example, meaning) in examples {
        let pad = " ".repeat(width - example.chars().count());
        out.push_str(&format!(
            "  {literal}{example}{literal:#}{pad}  {meaning}\n"
        ));
    }
    let exits = match name {
        "kill" | "stop" | "freeze" | "thaw" | "unstick" => Some(TARGETED_EXITS),
        "free" => Some(FREE_EXITS),
        "stuck" | "orphans" => Some(SEARCH_EXITS),
        "wait" => Some(WAIT_EXITS),
        _ => None,
    };
    if let Some(exits) = exits {
        out.push('\n');
        out.push_str(&render_sections(&[&Section {
            header: "Exit codes:",
            rows: exits,
        }]));
    } else {
        out.push('\n');
    }
    out.push_str(&more(&format!("https://proc-cli.com/commands/{name}")));
    StyledStr::from(out)
}

/// Semantic CLI tool for process management. Target by port, PID, name or path.
#[derive(Parser)]
#[command(name = "proc")]
#[command(author, version = VERSION_INFO, about, long_about = None)]
#[command(propagate_version = true)]
#[command(styles = STYLES)]
#[command(after_help = short_footer())]
#[command(after_long_help = long_footer())]
pub struct Cli {
    /// The subcommand; none prints the orientation text
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Every `proc` subcommand.
#[derive(Subcommand)]
pub enum Commands {
    /// Port/process lookup (bidirectional)
    #[command(visible_alias = ":")]
    #[command(after_help = command_footer("on"))]
    On(OnCommand),

    /// Find processes by file path
    #[command(visible_alias = "f")]
    #[command(after_help = command_footer("for"))]
    For(ForCommand),

    /// Filter processes by name
    #[command(visible_alias = "b")]
    #[command(after_help = command_footer("by"))]
    By(ByCommand),

    /// Find processes running in a directory or a folder below it
    #[command(after_help = command_footer("in"))]
    In(InCommand),

    /// List processes
    #[command(visible_aliases = ["l", "ps"])]
    #[command(after_help = command_footer("list"))]
    List(ListCommand),

    /// Show detailed process information
    #[command(visible_aliases = ["i", "inspect"])]
    #[command(after_help = command_footer("info"))]
    Info(InfoCommand),

    /// List all listening TCP ports
    #[command(visible_alias = "p")]
    #[command(after_help = command_footer("ports"))]
    Ports(PortsCommand),

    /// Kill process(es) forcefully
    #[command(visible_alias = "k")]
    #[command(after_help = command_footer("kill"))]
    Kill(KillCommand),

    /// Stop process(es) gracefully
    #[command(visible_alias = "s")]
    #[command(after_help = command_footer("stop"))]
    Stop(StopCommand),

    /// Show process tree
    #[command(visible_alias = "t")]
    #[command(after_help = command_footer("tree"))]
    Tree(TreeCommand),

    /// Watch processes in real-time
    #[command(visible_aliases = ["w", "top"])]
    #[command(after_help = command_footer("watch"))]
    Watch(WatchCommand),

    /// Find stuck/hung processes
    #[command(visible_alias = "x")]
    #[command(after_help = command_footer("stuck"))]
    Stuck(StuckCommand),

    /// Attempt to recover stuck processes
    ///
    /// Sends SIGCONT, then SIGINT (as Ctrl+C does, which ends most programs) to a
    /// process still busy; --force adds SIGTERM, then SIGKILL, for one still stuck.
    #[command(visible_alias = "u")]
    #[command(after_help = command_footer("unstick"))]
    Unstick(UnstickCommand),

    /// Freeze (pause) process(es) with SIGSTOP
    #[command(after_help = command_footer("freeze"))]
    Freeze(FreezeCommand),

    /// Resume frozen process(es) with SIGCONT
    #[command(after_help = command_footer("thaw"))]
    Thaw(ThawCommand),

    /// Find orphaned processes
    ///
    /// Lists the processes whose parent is PID 1, apart from system daemons.
    /// On macOS that includes every app launchd started, so narrow the list with
    /// --in or --by, and check each process before --kill.
    #[command(visible_alias = "o")]
    #[command(after_help = command_footer("orphans"))]
    Orphans(OrphansCommand),

    /// Trace why a port is busy or show process ancestry
    #[command(after_help = command_footer("why"))]
    Why(WhyCommand),

    /// Free port(s): stop what holds them and check they are free
    ///
    /// Asks each holder to exit (SIGTERM), force-kills (SIGKILL) whatever is still
    /// running after up to 5 seconds (or --wait, if shorter), then checks the port.
    #[command(after_help = command_footer("free"))]
    Free(FreeCommand),

    /// Wait for process(es) to exit
    #[command(after_help = command_footer("wait"))]
    Wait(WaitCommand),

    /// Generate shell completions
    #[command(hide = true)]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Generate the man page, or with --dir one per command
    #[command(hide = true)]
    Manpage {
        /// Write proc.1 and a page per command (proc-kill.1, ...) into this
        /// directory, made if it isn't there, instead of printing proc.1
        #[arg(long, value_name = "DIR")]
        dir: Option<std::path::PathBuf>,
    },
}
