#!/usr/bin/env bash
# Install the locally built PicoMado server and the current public pic2gray4px.
# Does not stop/start a service or touch DB/plugin data.
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
server="$repo_root/service/target/release/picomado-server"
bin_dir=${PICOMADO_INSTALL_BIN_DIR:-"$HOME/.cargo/bin"}
[[ -f "$server" && -x "$server" ]] || { echo 'Build the PicoMado server first' >&2; exit 2; }
[[ "$bin_dir" = /* ]] || { echo 'Install bin directory must be absolute' >&2; exit 2; }
mkdir -p "$bin_dir"
stage=$(mktemp -d "$bin_dir/.picomado-install.XXXXXXXX")
install_started=0
committed=0
old_server=0
old_cli=0
old_origin=0
old_hash=0
rollback() {
  if [[ $install_started == 1 && $committed == 0 ]]; then
    if [[ $old_server == 1 ]]; then cp -p "$stage/old-server" "$bin_dir/picomado-server"; else rm -f "$bin_dir/picomado-server"; fi
    if [[ $old_cli == 1 ]]; then cp -p "$stage/old-cli" "$bin_dir/pic2gray4px"; else rm -f "$bin_dir/pic2gray4px"; fi
    if [[ $old_origin == 1 ]]; then cp -p "$stage/old-origin" "$bin_dir/pic2gray4px-source.toml"; else rm -f "$bin_dir/pic2gray4px-source.toml"; fi
    if [[ $old_hash == 1 ]]; then cp -p "$stage/old-hash" "$bin_dir/pic2gray4px-sha256.txt"; else rm -f "$bin_dir/pic2gray4px-sha256.txt"; fi
  fi
  rm -rf "$stage"
}
trap rollback EXIT
cargo install --git https://github.com/tanep3/pic2gray4px.git --branch main --locked --force --root "$stage/cargo"
[[ -x "$stage/cargo/bin/pic2gray4px" && -f "$stage/cargo/.crates.toml" ]] || { echo 'pic2gray4px build incomplete' >&2; exit 2; }
install -m 755 "$server" "$stage/new-server"
install -m 755 "$stage/cargo/bin/pic2gray4px" "$stage/new-cli"
install -m 644 "$stage/cargo/.crates.toml" "$stage/new-origin"
printf '%s  pic2gray4px\n' "$(sha256sum "$stage/new-cli" | cut -d' ' -f1)" > "$stage/new-hash"
[[ ! -e "$bin_dir/picomado-server" ]] || { cp -p "$bin_dir/picomado-server" "$stage/old-server"; old_server=1; }
[[ ! -e "$bin_dir/pic2gray4px" ]] || { cp -p "$bin_dir/pic2gray4px" "$stage/old-cli"; old_cli=1; }
[[ ! -e "$bin_dir/pic2gray4px-source.toml" ]] || { cp -p "$bin_dir/pic2gray4px-source.toml" "$stage/old-origin"; old_origin=1; }
[[ ! -e "$bin_dir/pic2gray4px-sha256.txt" ]] || { cp -p "$bin_dir/pic2gray4px-sha256.txt" "$stage/old-hash"; old_hash=1; }
install_started=1
mv -f "$stage/new-server" "$bin_dir/picomado-server"
mv -f "$stage/new-cli" "$bin_dir/pic2gray4px"
mv -f "$stage/new-origin" "$bin_dir/pic2gray4px-source.toml"
mv -f "$stage/new-hash" "$bin_dir/pic2gray4px-sha256.txt"
committed=1
printf 'Installed server and pic2gray4px in %s\n' "$bin_dir"
sha256sum "$bin_dir/picomado-server" "$bin_dir/pic2gray4px"
cat "$bin_dir/pic2gray4px-source.toml"
