{ lib, pkgs, ... }:

{
  packages = [
    pkgs.git
  ] ++ lib.optionals pkgs.stdenv.isLinux (with pkgs; [
    xorg.libxcb
  ]) ++ lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin; [
    apple_sdk.frameworks.AppKit
    apple_sdk.frameworks.Foundation
    libobjc
  ]);

  languages.rust.enable = true;
}
