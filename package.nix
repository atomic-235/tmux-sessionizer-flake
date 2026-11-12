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
      x86_64-linux = "tmux-sessionizer-linux-x86_64";
    }
    .${system} or (throw "tmux-sessionizer: unsupported system ${system}");

  url = "https://github.com/atomic-235/tmux-sessionizer-flake/releases/download/v${data.version}/${asset}";

  # Fixed-output fetch at build time. The static glibc binary still embeds
  # glibc's locale/gconv/getconf paths, which makes Nix's reference check
  # reject it unless the check is waived; fetchurl does not accept
  # unsafeDiscardReferences, hence the flat stdenv.mkDerivation.
  binary = stdenv.mkDerivation {
    name = "tmux-sessionizer-${data.version}-${asset}";

    outputHashMode = "flat";
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

  src = binary;
  dontUnpack = true;
  dontStrip = true;

  installPhase = ''
    runHook preInstall
    install -Dm755 "$src" "$out/bin/tmux-sessionizer"
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
