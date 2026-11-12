{
  lib,
  stdenv,
  stdenvNoCC,
  curl,
  cacert,
}:

let
  data = builtins.fromJSON (builtins.readFile ./version.json);
  system = stdenvNoCC.hostPlatform.system;

  asset =
    {
      x86_64-linux = "tmux-sessionizer-linux-x86_64.tar.gz";
    }
    .${system} or (throw "tmux-sessionizer: unsupported system ${system}");

  url = "https://github.com/atomic-235/tmux-sessionizer-flake/releases/download/v${data.version}/${asset}";

  # Fixed-output fetch at build time. Recursive hashing (not flat) is required
  # because the static glibc binary embeds glibc's locale/gconv/getconf paths,
  # and Nix only honours unsafeDiscardReferences in recursive mode.
  tarball = stdenv.mkDerivation {
    name = "tmux-sessionizer-${data.version}-${asset}";

    outputHashMode = "recursive";
    outputHashAlgo = "sha256";
    outputHash = data.assets.${system};
    unsafeDiscardReferences = true;

    nativeBuildInputs = [ curl ];

    buildCommand = ''
      export SSL_CERT_FILE=${cacert}/etc/ssl/certs/ca-bundle.crt
      curl -fsSL --cacert "$SSL_CERT_FILE" -o "$out" ${lib.escapeShellArg url}
    '';
  };
in
stdenvNoCC.mkDerivation {
  pname = "tmux-sessionizer";
  version = data.version;

  src = tarball;
  # The archive contains a single file, not a directory.
  sourceRoot = ".";

  installPhase = ''
    runHook preInstall
    install -Dm755 tmux-sessionizer "$out/bin/tmux-sessionizer"
    runHook postInstall
  '';

  meta = with lib; {
    description = "Fuzzy-select a project directory and open it in tmux (static release binary)";
    homepage = "https://github.com/atomic-235/tmux-sessionizer-flake";
    license = licenses.mit;
    platforms = [ "x86_64-linux" ];
    mainProgram = "tmux-sessionizer";
  };
}
