{ getSystemIgnoreWarning, ... }:
let
  withSystemIgnoreWarning =
    system: f:
    f (getSystemIgnoreWarning system).allModuleArgs;
in
{
  flake.overlays.default = final: prev:
    withSystemIgnoreWarning prev.stdenv.hostPlatform.system (
      { config, ... }: {
        anyrunPlugins = (prev.anyrunPlugins or { }) // {
          cliphist = config.packages.default;
        };
      }
    );
}
