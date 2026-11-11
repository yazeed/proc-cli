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
            ("proc free :3000", "Kill process, verify port freed"),
            ("proc free :3000 :8080 --yes", "Free several ports"),
        ],
    ),
    (
        "Diagnostics",
        &[
            ("proc why :3000", "Why is port 3000 busy? (ancestry)"),
            ("proc orphans", "Find orphaned processes"),
            ("proc orphans --kill", "Find and kill orphans"),
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
    On(OnCommand),

    /// Find processes by file path
    #[command(visible_alias = "f")]
    For(ForCommand),

    /// Filter processes by name
    #[command(visible_alias = "b")]
    By(ByCommand),

    /// Find processes running in a directory or a folder below it
    In(InCommand),

    /// List processes
    #[command(visible_aliases = ["l", "ps"])]
    List(ListCommand),

    /// Show detailed process information
    #[command(visible_aliases = ["i", "inspect"])]
    Info(InfoCommand),

    /// List all listening TCP ports
    #[command(visible_alias = "p")]
    Ports(PortsCommand),

    /// Kill process(es) forcefully
    #[command(visible_alias = "k")]
    Kill(KillCommand),

    /// Stop process(es) gracefully
    #[command(visible_alias = "s")]
    Stop(StopCommand),

    /// Show process tree
    #[command(visible_alias = "t")]
    Tree(TreeCommand),

    /// Watch processes in real-time
    #[command(visible_aliases = ["w", "top"])]
    Watch(WatchCommand),

    /// Find stuck/hung processes
    #[command(visible_alias = "x")]
    Stuck(StuckCommand),

    /// Attempt to recover stuck processes
    ///
    /// Sends SIGCONT, then SIGINT (as Ctrl+C does, which ends most programs) to a
    /// process still busy; --force adds SIGTERM, then SIGKILL, for one still stuck.
    #[command(visible_alias = "u")]
    Unstick(UnstickCommand),

    /// Freeze (pause) process(es) with SIGSTOP
    Freeze(FreezeCommand),

    /// Resume frozen process(es) with SIGCONT
    Thaw(ThawCommand),

    /// Find orphaned processes
    ///
    /// Lists the processes whose parent is PID 1, apart from system daemons.
    /// On macOS that includes every app launchd started, so narrow the list with
    /// --in or --by, and check each process before --kill.
    #[command(visible_alias = "o")]
    Orphans(OrphansCommand),

    /// Trace why a port is busy or show process ancestry
    Why(WhyCommand),

    /// Free port(s): stop what holds them and check they are free
    ///
    /// Asks each holder to exit (SIGTERM), force-kills (SIGKILL) whatever is still
    /// running after up to 5 seconds (or --wait, if shorter), then checks the port.
    Free(FreeCommand),

    /// Wait for process(es) to exit
    Wait(WaitCommand),

    /// Generate shell completions
    #[command(hide = true)]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Generate man page
    #[command(hide = true)]
    Manpage,
}
