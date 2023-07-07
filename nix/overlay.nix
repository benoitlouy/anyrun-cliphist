{ withSystem, ... }: {
  flake.overlays.default = final: prev:
    withSystem prev.stdenv.hostPlatform.system (
      { config, ... }: {
        anyrun-cliphist = config.packages.default;
      }
    );
}
