# E2E smoke test: aivcsd starts and the aivcs CLI can init + snapshot.
{ pkgs, aivcsPackage, aivcsdPackage }:

pkgs.testers.nixosTest {
  name = "aivcs-wsl-e2e";

  nodes.machine = {
    config,
    pkgs,
    lib,
    ...
  }: {
    imports = [ ../modules/aivcsd.nix ];

    services.aivcsd = {
      enable = true;
      package = aivcsdPackage;
    };

    environment.systemPackages = [ aivcsPackage ];

    system.stateVersion = "25.05";
  };

  testScript = ''
    machine.wait_for_unit("aivcsd.service")

    machine.succeed("mkdir -p /tmp/aivcs-demo")
    machine.succeed("bash -lc 'cd /tmp/aivcs-demo && aivcs init'")
    machine.succeed("echo '{\"step\":1}' > /tmp/aivcs-demo/state.json")
    machine.succeed("bash -lc 'cd /tmp/aivcs-demo && aivcs snapshot --state state.json --message e2e'")
    machine.succeed("bash -lc 'cd /tmp/aivcs-demo && aivcs log | grep -q e2e'")
  '';
}
