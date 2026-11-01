#!/usr/bin/env node

const https = require("https");
const fs = require("fs");
const os = require("os");
const path = require("path");
const crypto = require("crypto");
const { execFileSync } = require("child_process");

const VERSION = require("./package.json").version;
const REPO = "yazeed/proc-cli";

// Only follow redirects to trusted GitHub domains.
// GitHub serves release downloads from release-assets.githubusercontent.com;
// the older hosts are kept for any redirect that still uses them.
const ALLOWED_HOSTS = [
  "github.com",
  "release-assets.githubusercontent.com",
  "objects.githubusercontent.com",
  "github-releases.githubusercontent.com",
];
const MAX_REDIRECTS = 5;

function getPlatformInfo() {
  const platform = process.platform;
  const arch = process.arch;

  let os, archName, archiveExt;

  switch (platform) {
    case "darwin":
      os = "darwin";
      archiveExt = ".tar.gz";
      break;
    case "linux":
      os = "linux";
      archiveExt = ".tar.gz";
      break;
    case "freebsd":
    case "netbsd":
      // Releases have an x86_64 build only for these
      if (arch !== "x64") {
        throw new Error(
          `No ${platform} ${arch} build in the releases; build it with: cargo install proc-cli`
        );
      }
      os = platform;
      archiveExt = ".tar.gz";
      break;
    case "win32":
      os = "windows";
      archiveExt = ".exe.zip";
      break;
    default:
      throw new Error(`Unsupported platform: ${platform}`);
  }

  switch (arch) {
    case "x64":
      archName = "x86_64";
      break;
    case "arm64":
      // Releases have no Windows Arm build; Windows on Arm runs the x64 one
      archName = platform === "win32" ? "x86_64" : "aarch64";
      break;
    default:
      throw new Error(`Unsupported architecture: ${arch}`);
  }

  const binaryName = `proc-${os}-${archName}${platform === "win32" ? ".exe" : ""}`;
  const archiveName = `proc-${os}-${archName}${archiveExt}`;

  return { os, archName, binaryName, archiveName, platform };
}

/** Path of the installed binary inside `dir` */
function binaryPath(dir) {
  return path.join(dir, process.platform === "win32" ? "proc.exe" : "proc");
}

/**
 * Per-user, per-version fallback location, for when the package directory
 * is not writable (e.g. a global install owned by root).
 */
function cacheDir() {
  const base =
    process.platform === "win32"
      ? process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local")
      : process.env.XDG_CACHE_HOME || path.join(os.homedir(), ".cache");
  return path.join(base, "proc-cli", VERSION);
}

function download(url, redirectDepth = 0) {
  return new Promise((resolve, reject) => {
    if (redirectDepth > MAX_REDIRECTS) {
      return reject(new Error("Too many redirects"));
    }

    const parsed = new URL(url);
    if (!ALLOWED_HOSTS.includes(parsed.hostname)) {
      return reject(
        new Error(`Redirect to untrusted host: ${parsed.hostname}`)
      );
    }

    https
      .get(url, (response) => {
        if (response.statusCode === 302 || response.statusCode === 301) {
          download(response.headers.location, redirectDepth + 1)
            .then(resolve)
            .catch(reject);
          return;
        }
        if (response.statusCode !== 200) {
          reject(new Error(`HTTP ${response.statusCode}`));
          return;
        }
        const chunks = [];
        response.on("data", (chunk) => chunks.push(chunk));
        response.on("end", () => resolve(Buffer.concat(chunks)));
        response.on("error", reject);
      })
      .on("error", reject);
  });
}

function verifyChecksum(buffer, expectedHash, archiveName) {
  const actualHash = crypto.createHash("sha256").update(buffer).digest("hex");
  if (actualHash !== expectedHash) {
    throw new Error(
      `Checksum mismatch for ${archiveName}!\n` +
        `  Expected: ${expectedHash}\n` +
        `  Actual:   ${actualHash}\n` +
        `  The download may be corrupted or tampered with.`
    );
  }
}

function parseChecksums(raw, archiveName) {
  const lines = raw.toString().split("\n");
  for (const line of lines) {
    // Format: "hash  filename" (sha256sum output)
    const parts = line.trim().split(/\s+/);
    if (parts.length >= 2 && parts[1] === archiveName) {
      return parts[0];
    }
  }
  throw new Error(
    `Checksum not found for ${archiveName} in checksums.txt`
  );
}

/**
 * Download, verify, and install the proc binary into `binDir`.
 * Progress goes to stderr so `proc --json` output on stdout stays clean
 * when this runs on first use. Returns the installed binary path.
 */
async function install(binDir) {
  const { binaryName, archiveName, platform } = getPlatformInfo();
  const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${archiveName}`;
  const checksumsUrl = `https://github.com/${REPO}/releases/download/v${VERSION}/checksums.txt`;
  const binPath = binaryPath(binDir);

  console.error(`Downloading proc v${VERSION}...`);
  console.error(`  URL: ${url}`);

  // Download checksums and archive in parallel
  const [checksumsRaw, buffer] = await Promise.all([
    download(checksumsUrl),
    download(url),
  ]);

  // Verify integrity before extracting
  const expectedHash = parseChecksums(checksumsRaw, archiveName);
  verifyChecksum(buffer, expectedHash, archiveName);
  console.error(`  Checksum verified: ${expectedHash.slice(0, 16)}...`);

  // Extract into a private temp directory, then move the binary into place,
  // so two first runs at the same time can't corrupt each other's files
  fs.mkdirSync(binDir, { recursive: true });
  const workDir = fs.mkdtempSync(path.join(binDir, ".proc-install-"));
  try {
    const tmpFile = path.join(workDir, archiveName);
    fs.writeFileSync(tmpFile, buffer);

    if (archiveName.endsWith(".tar.gz")) {
      // Use execFileSync with argument array — no shell injection possible
      execFileSync("tar", ["-xzf", tmpFile, "-C", workDir], {
        stdio: "pipe",
      });
    } else if (archiveName.endsWith(".zip")) {
      // Use execFileSync with argument array — no shell injection possible
      execFileSync(
        "powershell",
        [
          "-Command",
          `Expand-Archive -Force -Path '${tmpFile.replace(/'/g, "''")}' -DestinationPath '${workDir.replace(/'/g, "''")}'`,
        ],
        { stdio: "pipe" }
      );
    }

    const extractedBin = path.join(workDir, binaryName);
    if (!fs.existsSync(extractedBin)) {
      throw new Error(`${binaryName} not found in ${archiveName}`);
    }

    // Make executable
    if (platform !== "win32") {
      fs.chmodSync(extractedBin, 0o755);
    }
    fs.renameSync(extractedBin, binPath);
  } finally {
    fs.rmSync(workDir, { recursive: true, force: true });
  }

  console.error(`  Installed to: ${binPath}`);
  return binPath;
}

// The package has no install script: run.js downloads the binary on first use.
// `node install.js` still pre-downloads it, e.g. for an image that runs offline.
if (require.main === module) {
  install(__dirname)
    .then(() => console.error("Done!"))
    .catch((err) => {
      console.error(`Could not download proc during install: ${err.message}`);
      console.error("It will be downloaded the first time you run `proc`.");
    });
}

module.exports = { install, binaryPath, cacheDir };
