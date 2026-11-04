# Builds package.nix (the file nixpkgs carries as pkgs/by-name/pr/proc-cli)
# against the nixpkgs in this repo's flake.lock, with the maintainer entry
# added in case that nixpkgs doesn't have it yet:
#   nix build -L --impure -f pkg/nixpkgs/build.nix
# CI runs it on Linux and macOS; without Nix, in the nixos/nix image with the
# repo mounted (RELEASE.md, "After the tag: nixpkgs").
# It fetches only that nixpkgs: builtins.getFlake on the repository would copy
# all of it into the store, a local target/ included (19 GB, 2026-11-04)
let
  lock = builtins.fromJSON (builtins.readFile ../../flake.lock);
  nixpkgs = builtins.fetchTree lock.nodes.${lock.nodes.root.inputs.nixpkgs}.locked;
  pkgs = import nixpkgs { };
  lib = pkgs.lib // {
    maintainers = {
      yazeed = {
        github = "yazeed";
        githubId = 191483;
        name = "Yazeed Al Oyoun";
      };
    }
    // pkgs.lib.maintainers;
  };
in
pkgs.callPackage ./package.nix { inherit lib; }
