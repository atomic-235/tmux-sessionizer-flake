{
  lib,
  rustPlatform,
  glibc,
  ...
}:

# Built by CI, not by consumers: the resulting binary is published as the
# release asset that package.nix fetches.
let
  data = builtins.fromJSON (builtins.readFile ./version.json);
in
rustPlatform.buildRustPackage {
  pname = "tmux-sessionizer";
  version = data.version;

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: _: baseNameOf path != "target";
  };
  cargoLock.lockFile = ./Cargo.lock;

  # static-pie against glibc: the published asset must not carry a /nix/store
  # interpreter, so it runs wherever it is downloaded.
  RUSTFLAGS = "-C target-feature=+crt-static";
  buildInputs = [ glibc.static ];

  doCheck = false;

  meta = with lib; {
    description = "Fuzzy-select a project directory and open it in tmux";
    homepage = "https://github.com/atomic-235/tmux-sessionizer-flake";
    license = licenses.mit;
    platforms = [ "x86_64-linux" ];
    mainProgram = "tmux-sessionizer";
  };
}
