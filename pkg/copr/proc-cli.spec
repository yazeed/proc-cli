# Fedora COPR package (yazeed/proc-cli). The crates come from a vendor
# tarball, so the build needs no network. pkg/copr/srpm.sh makes the
# tarball and the source RPM for a version. There is no check section: the
# tests start servers, terminals and child processes, which the build
# sandbox does not allow; CI runs them on every commit instead.
#
# Amazon Linux has a new enough Rust but an older generation of Fedora's Rust
# RPM macros (rust-packaging 21, without cargo-rpm-macros 25), so there proc
# builds with cargo directly, from the same vendored crates.
Name:           proc-cli
Version:        2.2.0
Release:        1%{?dist}
Summary:        Semantic CLI tool for process management
# proc is MIT; the rest comes from the crates built into it on Linux
# (cargo tree -e normal,build --target x86_64-unknown-linux-gnu).
License:        MIT AND MPL-2.0 AND Unicode-3.0 AND (Apache-2.0 OR MIT) AND (Unlicense OR MIT) AND (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT)
URL:            https://proc-cli.com
Source0:        https://github.com/yazeed/proc-cli/archive/refs/tags/v%{version}/%{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.xz

%if 0%{?amzn}
BuildRequires:  cargo
%global debug_package %{nil}
%global procbin target/release/proc
%else
BuildRequires:  cargo-rpm-macros >= 25
%global procbin target/rpm/proc
%endif
BuildRequires:  rust >= 1.88
BuildRequires:  gcc

%description
Semantic CLI tool for process management. Target by port, PID, name or path.
proc finds what listens on a port, runs from a directory or holds a file
open, and stops, frees, pauses or waits for it, with a preview before any
change and JSON output for scripts.

%prep
%autosetup -n proc-cli-%{version} -p1 -a1
%if 0%{?amzn}
cat >> .cargo/config.toml << 'END'

[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
END
%else
%cargo_prep -v vendor
%endif

%build
%if 0%{?amzn}
cargo build --release --offline --locked
%else
%cargo_build
%{cargo_license_summary}
%{cargo_license} > LICENSE.dependencies
%{cargo_vendor_manifest}
%endif

%install
install -Dpm 0755 %{procbin} %{buildroot}%{_bindir}/proc
%{procbin} manpage --dir man
install -d %{buildroot}%{_mandir}/man1
install -pm 0644 man/*.1 %{buildroot}%{_mandir}/man1/
%{procbin} completions bash > proc.bash
%{procbin} completions zsh > _proc
%{procbin} completions fish > proc.fish
install -Dpm 0644 proc.bash %{buildroot}%{bash_completions_dir}/proc
install -Dpm 0644 _proc %{buildroot}%{zsh_completions_dir}/_proc
install -Dpm 0644 proc.fish %{buildroot}%{fish_completions_dir}/proc.fish

%files
%license LICENSE
%if ! 0%{?amzn}
%license LICENSE.dependencies
%license cargo-vendor.txt
%endif
%doc README.md CHANGELOG.md
%{_bindir}/proc
%{_mandir}/man1/proc*.1*
%{bash_completions_dir}/proc
%{zsh_completions_dir}/_proc
%{fish_completions_dir}/proc.fish

%changelog
* Mon Nov 02 2026 Yazeed Al Oyoun <me@yazeed.com> - 2.2.0-1
- First COPR build
