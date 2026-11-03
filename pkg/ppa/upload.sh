#!/bin/sh
# Signs the source packages pkg/ppa/build-source.sh made, with your key, and
# uploads them to ppa:yazeedaloyoun/proc-cli:
#   pkg/ppa/upload.sh DIR [--sign-only]
# Runs on macOS or Linux with gpg, openssl and python3; gpg asks for the key's
# passphrase. It signs as debsign does (the .dsc, its checksums in the
# .changes, then the .changes) and uploads by FTP into
# ~yazeedaloyoun/ubuntu/proc-cli/, as dput's default method does: Launchpad
# trusts an upload by its signature. Launchpad emails the result of each.
# Each package's files go in one FTP session, as Launchpad takes the files of
# one session as one upload (txpkgupload, hooks.py: a directory per session,
# moved to the queue when it ends). A package that was sent gets a
# `.uploaded` mark, so running it again sends only what has not gone yet;
# what is signed already is not signed again. An upload that only names the
# tarballs (every release after the first, see build-source.sh) waits until
# the PPA has published them.
# --sign-only signs and stops, to check the files before anything is sent.
set -eu

dir=$(cd "${1:?usage: upload.sh DIR [--sign-only]}" && pwd)
sign_only=${2:-}
key=${PPA_KEY:-227B9E7E7C22B6E88774325184D7BBF5E38CCC4D}
incoming='~yazeedaloyoun/ubuntu/proc-cli/'
pool=https://ppa.launchpadcontent.net/yazeedaloyoun/proc-cli/ubuntu/pool/main/p/proc-cli

digest() { openssl dgst "-$1" -r "$2" | cut -d' ' -f1; }

# sign FILE OUTPUT, with the key on this machine (gpg asks for its passphrase)
sign() { gpg --local-user "$key" --clearsign --output "$2" "$1"; }

for changes in "$dir"/*_source.changes; do
    if [ -e "$changes.uploaded" ]; then
        echo "sent already: $(basename "$changes")"
        continue
    fi
    dsc=$(awk '/^Files:/ { files = 1; next } files && $NF ~ /\.dsc$/ { print $NF; exit }' "$changes")

    # An upload that only names the tarballs (all but the first release's)
    # needs them published in the PPA already, or Launchpad rejects it
    waiting=$(awk '/^Files:/ { files = 1; next } /^[^ ]/ { files = 0 } files && $NF ~ /\.orig/ { print $NF }' "$dir/$dsc" |
        while read -r tarball; do
            if ! grep -q " $tarball\$" "$changes" &&
                ! curl -fsI -o /dev/null "$pool/$tarball"; then
                printf ' %s' "$tarball"
            fi
        done)
    if [ -n "$waiting" ]; then
        echo "not yet: $(basename "$changes") needs$waiting published in the PPA first; run this again once the first upload is published"
        continue
    fi

    if ! grep -q 'BEGIN PGP SIGNED MESSAGE' "$dir/$dsc"; then
        sign "$dir/$dsc" "$dir/$dsc.signed"
        mv "$dir/$dsc.signed" "$dir/$dsc"
    fi

    # the signed .dsc has new checksums and size: write them into the .changes
    if ! grep -q 'BEGIN PGP SIGNED MESSAGE' "$changes"; then
        size=$(wc -c < "$dir/$dsc" | tr -d ' ')
        awk -v file="$dsc" -v size="$size" \
            -v md5="$(digest md5 "$dir/$dsc")" \
            -v sha1="$(digest sha1 "$dir/$dsc")" \
            -v sha256="$(digest sha256 "$dir/$dsc")" '
            /^[^ ]/ { section = $1 }
            /^ / && $NF == file {
                if (section == "Checksums-Sha1:") { print " " sha1 " " size " " file; next }
                if (section == "Checksums-Sha256:") { print " " sha256 " " size " " file; next }
                if (section == "Files:") { print " " md5 " " size " " $3 " " $4 " " file; next }
            }
            { print }' "$changes" > "$changes.new"
        mv "$changes.new" "$changes"
        sign "$changes" "$changes.signed"
        mv "$changes.signed" "$changes"
    fi
    if [ "$sign_only" = --sign-only ]; then
        echo "signed $(basename "$changes")"
        continue
    fi

    # the files it names, then the .changes, which starts the build: all in
    # one session, tried up to three times
    attempt=1
    until "${PYTHON:-python3}" - "$dir" "$changes" "$incoming" <<'EOF'
import ftplib, os, sys
directory, changes, incoming = sys.argv[1:4]
files, listing = [], False
for line in open(changes):
    if line.startswith("Files:"):
        listing = True
    elif listing and line.startswith(" "):
        files.append(line.split()[-1])
    else:
        listing = False
files.append(os.path.basename(changes))
ftp = ftplib.FTP("ppa.launchpad.net", timeout=300)
ftp.login()
ftp.cwd(incoming)
for name in files:
    print("  sending", name, flush=True)
    with open(os.path.join(directory, name), "rb") as data:
        ftp.storbinary("STOR " + name, data)
ftp.quit()
EOF
    do
        if [ "$attempt" -ge 3 ]; then
            echo "upload.sh: $(basename "$changes") did not upload; run this again to retry" >&2
            exit 1
        fi
        attempt=$((attempt + 1))
        echo "retrying $(basename "$changes") (attempt $attempt of 3)"
        sleep 10
    done
    touch "$changes.uploaded"
    echo "sent $(basename "$changes")"
done
[ "$sign_only" = --sign-only ] && exit 0
echo "uploaded; Launchpad emails whether each was accepted, then builds it"
