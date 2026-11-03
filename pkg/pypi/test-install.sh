#!/bin/sh
# Installs proc-cli from the wheels in a fresh virtualenv, the way pip does for
# a user (but from a directory, not PyPI), and checks that `proc` runs and
# names the version. The release workflow runs it on each OS (test-pypi).
#
# usage: test-install.sh <version> <wheel dir> <push|dry run>
#   version   1.2.3, without the leading v
#   mode      "push" for a tag: proc must print exactly `proc <version>`.
#             Anything else is a dry run, whose binaries are -dev builds: the
#             line only has to start with `proc <version>`
set -eu

version=${1:?usage: test-install.sh VERSION WHEELDIR MODE}
wheels=${2:?usage: test-install.sh VERSION WHEELDIR MODE}
mode=${3:?usage: test-install.sh VERSION WHEELDIR MODE}

# Git Bash on Windows: python.exe, and the venv puts its commands in Scripts
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) py=python bin=Scripts exe=proc.exe ;;
  *) py=python3 bin=bin exe=proc ;;
esac

# A fresh temporary directory each run; a CI runner is thrown away after the job
venv=$(mktemp -d)
"$py" -m venv "$venv/v"
"$venv/v/$bin/python" -m pip install --no-index --find-links "$wheels" proc-cli

got=$("$venv/v/$bin/$exe" --version | head -n 1 | tr -d '\r')
echo "$got"
if [ "$mode" = push ]; then
  [ "$got" = "proc $version" ] || { echo "::error::proc --version printed '$got', expected 'proc $version'"; exit 1; }
else
  case "$got" in
    "proc $version"*) ;;
    *) echo "::error::proc --version printed '$got', expected it to start with 'proc $version'"; exit 1 ;;
  esac
fi
