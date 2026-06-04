{
  config,
  lib,
  pkgs,
  aivcsPackage,
  aivcsdPackage,
  ...
}:
{
  imports = [ ../modules/aivcsd.nix ];

  wsl.enable = true;
  system.stateVersion = "25.05";

  nix.settings = {
    experimental-features = [ "nix-command" "flakes" ];
    extra-substituters = [ "https://nix-cache.stevedores.org" ];
    extra-trusted-public-keys = [
      "stevedores-1:ZEtb+wHYNR/LDmMDhF3/EpRZDNma8exY2b1TGZ6uS2A="
      "stevedores-cache-1:bXLxkipycRWproIJnk8pPWNFdgVfeV+I2mJXCoW4/ag="
    ];
  };

  environment.systemPackages = [ aivcsPackage aivcsdPackage ];

  services.aivcsd = {
    enable = true;
    package = aivcsdPackage;
    settings = {
      SURREALDB_ENDPOINT = "memory";
    };
  };

  programs.bash.interactiveShellInit = lib.mkAfter ''
    echo "AIVCS NixOS-WSL — run 'aivcs env info' to validate your environment."
  '';
}
