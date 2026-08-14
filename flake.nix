{
  description = "Notiz - native Markdown knowledge base for Wayland";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      packages.${system}.default = pkgs.callPackage ./package.nix {};

      apps.${system}.default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/notiz";
      };

      devShells.${system}.default = import ./shell.nix { inherit pkgs; };
    };
}

