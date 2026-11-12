# tmux-sessionizer-flake

`tmux-sessionizer` — fuzzy-select a project directory and open it in tmux —
built static by CI and published as a single release binary.

The source lives in this repo. The Rust crate has no dependencies, and the
release binary is linked with `-C target-feature=+crt-static` against
`glibc.static`, so it carries no `/nix/store` interpreter and runs wherever it
is downloaded. The asset is a `.tar.gz` because static glibc still embeds
glibc's locale/gconv/getconf paths, and Nix only honours
`unsafeDiscardReferences` in recursive hashing mode.

## Packages

| Attribute | Contents |
|---|---|
| `packages.x86_64-linux.default` | the published static binary |
| `packages.x86_64-linux.tmux-sessionizer` | alias of `default` |
| `packages.x86_64-linux.static` | the `rustPlatform` build CI publishes |

`overlays.default` provides `pkgs.tmux-sessionizer`.

## Usage

```nix
{
  inputs.tmux-sessionizer-flake.url = "github:atomic-235/tmux-sessionizer-flake";
  inputs.tmux-sessionizer-flake.inputs.nixpkgs.follows = "nixpkgs";
}
```

```nix
nixpkgs.overlays = [ inputs.tmux-sessionizer-flake.overlays.default ];
# pkgs.tmux-sessionizer
```

## Updates

CI runs on every push to `main` that touches `Cargo.toml` or `src/`: it reads
the version from `Cargo.toml`, skips when `v<version>` already exists (override
with `workflow_dispatch` and `force=true`), builds the static binary, publishes
it, records the asset hash in `version.json`, verifies `.#default` fetches it,
then commits and force-moves the `v<version>` and `latest` tags.

Release the next version by bumping `version` in `Cargo.toml` and pushing.

Supported: `x86_64-linux`.
