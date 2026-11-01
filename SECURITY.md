# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 2.2.x   | :white_check_mark: |
| < 2.2   | :x:                |

1.x and 2.0.x are no longer supported. Upgrade to 2.1.x; see the [CHANGELOG](CHANGELOG.md) for what changed.

## Reporting a Vulnerability

We take security seriously. If you discover a security vulnerability in proc, please report it responsibly.

### How to Report

**DO NOT** create a public GitHub issue for security vulnerabilities.

Instead, report it privately through GitHub: **[Report a vulnerability](https://github.com/yazeed/proc-cli/security/advisories/new)**. Only the maintainer can see the report.

Include the following information:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### What to Expect

1. **Acknowledgment**: We will acknowledge receipt of your report within 48 hours.

2. **Investigation**: We will investigate the issue and determine its severity.

3. **Communication**: We will keep you informed of our progress.

4. **Resolution**: We will work to resolve the issue as quickly as possible.

5. **Credit**: We will credit you in the release notes (unless you prefer to remain anonymous).

### Response Timeline

- **Critical vulnerabilities**: Patch within 24-48 hours
- **High severity**: Patch within 1 week
- **Medium severity**: Patch within 2 weeks
- **Low severity**: Patch in next regular release

## Security Considerations

### Design Principles

proc is designed with security in mind:

1. **No Network Access**: proc only accesses local system information. It does not make network requests or phone home. (The npm package downloads the proc binary the first time you run it and verifies its checksum; the binary itself never does.)

2. **No Data Collection**: proc does not collect, store, or transmit any user data or telemetry.

3. **Minimal Privileges**: proc runs with the privileges of the invoking user. It does not require or request elevated privileges except when explicitly needed (e.g., killing processes owned by other users).

4. **Safe Defaults**: Destructive operations (like `kill`) require `--yes` or an interactive confirmation. `--json` does not count as confirmation: without `--yes` it exits 4 and changes nothing.

5. **Input Validation**: Inputs are validated (ports, ranges, signals, intervals), and proc starts helper programs with fixed argument lists, never through a shell.

6. **Memory Safety**: Written in Rust, which provides memory safety guarantees.

7. **Process data cannot control your terminal**: control characters in process names, command lines and paths are shown as visible escapes. Signals check that the PID still belongs to the same process before they are sent.

### Dependencies

We carefully vet all dependencies:

- Only well-maintained, widely-used crates
- Regular dependency audits with `cargo audit`
- Pinned versions via `Cargo.lock`

### Known Limitations

1. **Process Information Access**: On some systems, proc may not be able to access information about processes owned by other users without elevated privileges.

2. **Signal Sending**: Killing processes owned by other users requires appropriate permissions (typically root/sudo).

3. **Port Information**: Retrieving port information may require elevated privileges on some systems.

## Security Best Practices for Users

1. **Review before killing**: Always verify the processes you're about to kill, especially with `--cmd`, which matches any part of a command line.

2. **Use dry-run mode**: When uncertain, use `--dry-run` to see what would happen without making changes.

3. **Avoid running as root**: Run proc with regular user privileges when possible. Only use sudo when necessary.

4. **Keep proc updated**: Install security updates when available.

## Audit History

| Date | Type | Findings | Resolution |
|------|------|----------|------------|
| 2026-01-12 | v1.0.0 Release | N/A | N/A |

## Contact

For security concerns: [report a vulnerability](https://github.com/yazeed/proc-cli/security/advisories/new) (private)

For general questions: [GitHub Issues](https://github.com/yazeed/proc-cli/issues)
