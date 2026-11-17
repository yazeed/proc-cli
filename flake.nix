{
  description = "Semantic CLI tool for process management. Target by port, PID, name or path.";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  # nixpkgs dropped Intel Macs after 26.05 (26.11 release notes), and its
  # unstable branch refuses to evaluate for x86_64-darwin. 26.05 keeps them
  # until the end of 2026, so Intel Macs build from it; proc 2.5.0 is the last
  # release whose flake does
  inputs.nixpkgs-intel-mac.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-intel-mac,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      pkgsFor =
        system:
        if system == "x86_64-darwin" then
          nixpkgs-intel-mac.legacyPackages.${system}
        else
          nixpkgs.legacyPackages.${system};
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (pkgsFor system));
      cargoToml = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;
    in
    {
      packages = forAllSystems (pkgs: rec {
        proc-cli = pkgs.rustPlatform.buildRustPackage {
          pname = "proc-cli";
          # Read from Cargo.toml, so a release bumps the version in one place
          inherit (cargoToml) version;

          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.installShellFiles ];

          # The sandbox's PATH has only the build inputs, not /bin or /usr/sbin.
          # The tests run `ps` (Linux: procps; macOS: Apple's ps, darwin.ps),
          # and on macOS proc reads the port table with `lsof`
          nativeCheckInputs =
            pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.procps ]
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              pkgs.darwin.ps
              pkgs.lsof
            ];

          # The pseudo-terminal tests start proc in $HOME (portable-pty's
          # default working directory); the sandbox's HOME does not exist
          preCheck = ''
            export HOME=$(mktemp -d)
          '';

          # The port tests listen on 127.0.0.1, which macOS's sandbox refuses
          # (EPERM) without this
          __darwinAllowLocalNetworking = true;

          # The whole suite runs in the sandbox; never turn it off. If a test
          # the sandbox stops ever has to be skipped, do it by exact name in
          # `checkFlags = [ "--skip=<name>" ]`, with the reason beside it.
          doCheck = true;

          postInstall =
            pkgs.lib.optionalString (pkgs.stdenv.buildPlatform.canExecute pkgs.stdenv.hostPlatform)
              ''
                $out/bin/proc manpage --dir man
                installManPage man/*.1
                installShellCompletion --cmd proc \
                  --bash <($out/bin/proc completions bash) \
                  --zsh <($out/bin/proc completions zsh) \
                  --fish <($out/bin/proc completions fish)
              '';

          meta = {
            description = "Semantic CLI tool for process management. Target by port, PID, name or path.";
            homepage = "https://proc-cli.com";
            changelog = "https://github.com/yazeed/proc-cli/blob/v${cargoToml.version}/CHANGELOG.md";
            license = pkgs.lib.licenses.mit;
            mainProgram = "proc";
            platforms = pkgs.lib.platforms.linux ++ pkgs.lib.platforms.darwin;
          };
        };
        default = proc-cli;
      });

      # `nix flake check` builds the package, which runs `cargo test`
      checks = forAllSystems (pkgs: {
        proc-cli = self.packages.${pkgs.stdenv.hostPlatform.system}.proc-cli;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.proc-cli ];
          packages = with pkgs; [
            clippy
            rust-analyzer
            rustfmt
          ];
        };
      });
    };
}
