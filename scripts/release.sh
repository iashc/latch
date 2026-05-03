#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_ROOT="$ROOT_DIR/dist"
VERSION=""
PUBLISH=0
SKIP_INSTALL=0
TAP_PATH=""
TAP_COMMIT=0
TAP_PUSH=0
TARGETS="aarch64-apple-darwin"

usage() {
  cat <<'EOF'
Usage:
  scripts/release.sh [options]

Build release assets locally. By default this only writes files under dist/release/<tag>.

Options:
  --version <vX.Y.Z>       Release tag/version. Defaults to v<server/Cargo.toml version>.
  --targets <list>         Comma-separated Rust targets, or "current". Defaults to Apple silicon.
  --skip-install           Reuse existing node_modules and Rust targets.
  --publish                Create/upload GitHub Release assets with gh.
  --tap-path <path>        Copy generated Homebrew formula into an existing local tap checkout.
  --tap-commit             Commit the Homebrew formula update in --tap-path.
  --tap-push               Push the Homebrew tap commit. Implies --tap-commit.
  -h, --help               Show this help.

Environment:
  LATCH_HOMEBREW_TAP_PATH  Default tap checkout path when --tap-path is omitted.

Examples:
  scripts/release.sh --version v0.1.0
  scripts/release.sh --version v0.1.0 --publish
  LATCH_HOMEBREW_TAP_PATH=/absolute/path/to/homebrew-tap scripts/release.sh --version v0.1.0 --publish --tap-commit
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:-}"
      shift 2
      ;;
    --targets)
      TARGETS="${2:-}"
      shift 2
      ;;
    --skip-install)
      SKIP_INSTALL=1
      shift
      ;;
    --publish)
      PUBLISH=1
      shift
      ;;
    --tap-path)
      TAP_PATH="${2:-}"
      shift 2
      ;;
    --tap-commit)
      TAP_COMMIT=1
      shift
      ;;
    --tap-push)
      TAP_COMMIT=1
      TAP_PUSH=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

resolve_existing_dir() {
  local path="$1"
  if [[ ! -d "$path" ]]; then
    echo "Directory does not exist: $path" >&2
    exit 1
  fi
  (
    cd "$path"
    pwd -P
  )
}

validate_tap_path() {
  local path="$1"
  require_command git
  if ! git -C "$path" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "Tap path must be an existing git checkout: $path" >&2
    exit 1
  fi
}

current_cargo_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/server/Cargo.toml" | head -n 1
}

normalize_version() {
  local input="$1"
  if [[ "$input" == v* ]]; then
    echo "$input"
  else
    echo "v$input"
  fi
}

current_rust_target() {
  rustc -vV | awk '/^host:/ {print $2}'
}

split_targets() {
  if [[ "$TARGETS" == "current" ]]; then
    current_rust_target
  else
    echo "$TARGETS" | tr ',' '\n' | sed '/^$/d'
  fi
}

sha256_file() {
  shasum -a 256 "$1" | awk '{print $1}'
}

zip_dir_contents() {
  local source_dir="$1"
  local output_file="$2"
  (
    cd "$source_dir"
    zip -r -q "$output_file" .
  )
}

if [[ -z "$VERSION" ]]; then
  VERSION="$(normalize_version "$(current_cargo_version)")"
else
  VERSION="$(normalize_version "$VERSION")"
fi

if [[ -z "$TAP_PATH" && -n "${LATCH_HOMEBREW_TAP_PATH:-}" ]]; then
  TAP_PATH="$LATCH_HOMEBREW_TAP_PATH"
fi

if [[ "$TAP_COMMIT" -eq 1 && -z "$TAP_PATH" ]]; then
  echo "--tap-commit/--tap-push requires --tap-path or LATCH_HOMEBREW_TAP_PATH" >&2
  exit 1
fi

if [[ ! "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+ ]]; then
  echo "Version must look like vX.Y.Z, got: $VERSION" >&2
  exit 1
fi

require_command cargo
require_command npm
require_command node
require_command rustc
require_command shasum
require_command zip
require_command rsync

if [[ "$PUBLISH" -eq 1 ]]; then
  require_command gh
fi

if [[ -n "$TAP_PATH" ]]; then
  TAP_PATH="$(resolve_existing_dir "$TAP_PATH")"
  validate_tap_path "$TAP_PATH"
fi

OUT_DIR="$DIST_ROOT/release/$VERSION"
STAGING_DIR="$OUT_DIR/staging"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR" "$STAGING_DIR"

echo "==> Building Latch $VERSION"
echo "Output: $OUT_DIR"

CLI_ASSET_ENTRIES=()
while IFS= read -r target; do
  [[ -z "$target" ]] && continue
  if [[ "$SKIP_INSTALL" -eq 0 ]]; then
    rustup target add "$target"
  fi

  echo "==> Building CLI for $target"
  cargo build --manifest-path "$ROOT_DIR/server/Cargo.toml" --release --locked --target "$target"

  package_dir="$STAGING_DIR/latch-cli-$target"
  mkdir -p "$package_dir"
  cp "$ROOT_DIR/server/target/$target/release/latch" "$package_dir/latch"
  chmod 0755 "$package_dir/latch"
  tar -C "$package_dir" -czf "$OUT_DIR/latch-cli-$target.tar.gz" latch

  CLI_ASSET_ENTRIES+=("$target")
done < <(split_targets)

echo "==> Building Chrome extension"
(
  cd "$ROOT_DIR/browser"
  if [[ "$SKIP_INSTALL" -eq 0 ]]; then
    npm ci
  fi
  npm run compile
  npm run build
)
zip_dir_contents "$ROOT_DIR/browser/.output/chrome-mv3" "$OUT_DIR/latch-chrome-mv3.zip"

echo "==> Building Raycast extension"
(
  cd "$ROOT_DIR/raycast"
  if [[ "$SKIP_INSTALL" -eq 0 ]]; then
    npm ci
  fi
  npm run build
)
raycast_stage="$STAGING_DIR/latch-raycast"
raycast_package_name="$(node -p "require('$ROOT_DIR/raycast/package.json').name")"
raycast_build_dir="${XDG_CONFIG_HOME:-$HOME/.config}/raycast/extensions/$raycast_package_name"
if [[ ! -d "$raycast_build_dir" ]]; then
  echo "Raycast build output not found: $raycast_build_dir" >&2
  exit 1
fi
while IFS= read -r command_name; do
  [[ -z "$command_name" ]] && continue
  if [[ ! -f "$raycast_build_dir/$command_name.js" ]]; then
    echo "Raycast command executable missing: $raycast_build_dir/$command_name.js" >&2
    exit 1
  fi
done < <(node -e "const pkg = require('$ROOT_DIR/raycast/package.json'); for (const command of pkg.commands || []) console.log(command.name)")
mkdir -p "$raycast_stage"
rsync -a \
  --exclude node_modules \
  --exclude .git \
  --exclude .DS_Store \
  "$raycast_build_dir/" "$raycast_stage/"
zip_dir_contents "$raycast_stage" "$OUT_DIR/latch-raycast.zip"

echo "==> Writing release manifest"
manifest="$OUT_DIR/latch-release-manifest.json"
{
  echo "{"
  echo "  \"version\": \"${VERSION#v}\","
  echo "  \"assets\": ["
  first=1
  for target in "${CLI_ASSET_ENTRIES[@]}"; do
    asset="latch-cli-$target.tar.gz"
    if [[ "$first" -eq 0 ]]; then
      echo "    ,"
    fi
    first=0
    cat <<EOF
    {
      "kind": "cli",
      "target": "$target",
      "name": "$asset",
      "sha256": "$(sha256_file "$OUT_DIR/$asset")"
    }
EOF
  done
  for spec in "chrome:latch-chrome-mv3.zip" "raycast:latch-raycast.zip"; do
    kind="${spec%%:*}"
    asset="${spec#*:}"
    if [[ "$first" -eq 0 ]]; then
      echo "    ,"
    fi
    first=0
    cat <<EOF
    {
      "kind": "$kind",
      "name": "$asset",
      "sha256": "$(sha256_file "$OUT_DIR/$asset")"
    }
EOF
  done
  echo "  ]"
  echo "}"
} > "$manifest"

echo "==> Writing Homebrew formula"
formula_dir="$OUT_DIR/homebrew"
formula_file="$formula_dir/latch.rb"
mkdir -p "$formula_dir"
arm_asset="$OUT_DIR/latch-cli-aarch64-apple-darwin.tar.gz"
if [[ ! -f "$arm_asset" ]]; then
  echo "Homebrew formula requires the Apple silicon asset: $arm_asset" >&2
  exit 1
fi
arm_sha="$(sha256_file "$arm_asset")"
cat > "$formula_file" <<EOF
class Latch < Formula
  desc "Local-first bookmark service and personal clients"
  homepage "https://github.com/iashc/latch"
  url "https://github.com/iashc/latch/releases/download/$VERSION/latch-cli-aarch64-apple-darwin.tar.gz"
  sha256 "$arm_sha"
  version "${VERSION#v}"
  license "MIT"

  depends_on arch: :arm64
  depends_on :macos

  def install
    bin.install "latch"
  end

  test do
    assert_match "Local-first bookmark", shell_output("#{bin}/latch --help")
  end
end
EOF

if [[ -n "$TAP_PATH" ]]; then
  echo "==> Updating local Homebrew tap: $TAP_PATH"
  mkdir -p "$TAP_PATH/Formula"
  cp "$formula_file" "$TAP_PATH/Formula/latch.rb"
  if [[ "$TAP_COMMIT" -eq 1 ]]; then
    (
      cd "$TAP_PATH"
      git add Formula/latch.rb
      git commit -m "Update latch to $VERSION" || true
      if [[ "$TAP_PUSH" -eq 1 ]]; then
        git push
      fi
    )
  fi
fi

if [[ "$PUBLISH" -eq 1 ]]; then
  echo "==> Publishing GitHub Release $VERSION"
  mapfile -t release_assets < <(find "$OUT_DIR" -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.zip' -o -name 'latch-release-manifest.json' \) | sort)
  if gh release view "$VERSION" >/dev/null 2>&1; then
    gh release upload "$VERSION" "${release_assets[@]}" --clobber
  else
    gh release create "$VERSION" "${release_assets[@]}" --title "$VERSION" --notes "Latch $VERSION"
  fi
fi

echo "==> Done"
echo "Assets:"
find "$OUT_DIR" -maxdepth 1 -type f | sort
echo "Homebrew formula: $formula_file"
