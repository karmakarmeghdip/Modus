{ pkgs, lib, config, inputs, ... }:

{
  # https://devenv.sh/basics/
  # env.GREET = "devenv";

  packages = [
    pkgs.llvmPackages_23.llvm.dev
    pkgs.llvmPackages_23.libllvm
    pkgs.libffi
    pkgs.libxml2
    pkgs.zlib
    pkgs.ncurses
  ];

  env.LLVM_SYS_231_PREFIX = "${pkgs.llvmPackages_23.llvm.dev}";
  env.LIBRARY_PATH = lib.makeLibraryPath [
    pkgs.llvmPackages_23.libllvm
    pkgs.libffi
    pkgs.libxml2
    pkgs.zlib
    pkgs.ncurses
  ];

  # https://devenv.sh/languages/
  languages.rust.enable = true;

  # https://devenv.sh/processes/
  # processes.dev.exec = "${lib.getExe pkgs.watchexec} -n -- ls -la";

  # https://devenv.sh/services/
  # services.postgres.enable = true;

  # https://devenv.sh/scripts/
  # scripts.hello.exec = ''
  #   echo hello from $GREET
  # '';

  # https://devenv.sh/basics/
  # enterShell = ''
  #   hello         # Run scripts directly
  #   git --version # Use packages
  # '';

  # https://devenv.sh/tasks/
  # tasks = {
  #   "myproj:setup".exec = "mytool build";
  #   "devenv:enterShell".after = [ "myproj:setup" ];
  # };

  # https://devenv.sh/tests/
  # enterTest = ''
  #   echo "Running tests"
  #   git --version | grep --color=auto "${pkgs.git.version}"
  # '';

  # https://devenv.sh/git-hooks/
  # git-hooks.hooks.shellcheck.enable = true;

  # See full reference at https://devenv.sh/reference/options/
}
