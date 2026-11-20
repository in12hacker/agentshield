#!/usr/bin/env bash
# Local software smoke with the real executable-derived installation layout.
set -euo pipefail
source "${AS_CI_ENV:-${XDG_DATA_HOME:-$HOME/.local/share}/agentshield-ci/env.sh}"
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$repo"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/target}"
command -v cargo >/dev/null
cargo build -p as-middleware --example configuration_smoke --locked --offline
smoke_root=$(mktemp -d /tmp/as-configuration-smoke.XXXXXXXX)
trap 'rm -rf -- "$smoke_root"' EXIT
mkdir -p -- "$smoke_root/bin" "$smoke_root/config"
cp -- "$CARGO_TARGET_DIR/debug/examples/configuration_smoke" "$smoke_root/bin/as-service"
sed "s/uid: 0/uid: $(id -u)/" config/as.yaml > "$smoke_root/config/as.yaml"
cp -- config/rules.example.yaml "$smoke_root/config/rules.yaml"
"$smoke_root/bin/as-service"
