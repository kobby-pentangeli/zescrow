#!/usr/bin/env bash
#
# Zescrow developer toolchain setup.
#
# Installs (idempotently) the toolchains the project builds and tests against,
# pinned to the versions the workspace targets, so a contributor's machine and
# CI converge on the same setup:
#
#   - Rust       stable (build) + nightly (the `cargo +nightly fmt` formatter)
#   - RISC Zero  the rzup-managed zkVM toolchain (guest builds, r0vm, cargo-risczero)
#   - Solana     the Agave CLI (program deploy, local validator, keypairs)
#   - Anchor     via avm, pinned to the version in the Cargo manifests
#
# Usage:
#   scripts/setup.sh [--all] [--rust] [--risc0] [--solana] [--anchor]
#   scripts/setup.sh --verify        # print installed versions, install nothing
#   scripts/setup.sh --help
#
# Version pins (override via environment):
#   ANCHOR_VERSION   matches anchor-lang / anchor-client in the Cargo manifests
#   SOLANA_VERSION   Agave release channel ("stable") or an explicit tag
#
# Network access is required. Installers append to shell rc files; open a new
# shell (or `source` the noted env files) afterwards so PATH updates take effect.

set -euo pipefail

# --- Version pins -------------------------------------------------------------
# ANCHOR_VERSION must equal the anchor-lang / anchor-client versions pinned in
# client/Cargo.toml and the Solana program manifest. SOLANA_VERSION tracks the
# Agave line those Anchor crates target, so the toolchain matches the manifests.
ANCHOR_VERSION="${ANCHOR_VERSION:-1.0.2}"
SOLANA_VERSION="${SOLANA_VERSION:-v3.0.13}"

# --- Helpers ------------------------------------------------------------------
log()  { printf '\033[1;34m[setup]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[setup]\033[0m %s\n' "$*" >&2; }
have() { command -v "$1" >/dev/null 2>&1; }

ensure_rustup() {
  if ! have rustup; then
    log "Installing rustup (Rust toolchain manager)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    # shellcheck disable=SC1091
    source "${CARGO_HOME:-$HOME/.cargo}/env"
  fi
}

install_rust() {
  ensure_rustup
  log "Installing Rust stable + nightly with rustfmt/clippy..."
  rustup toolchain install stable --component rustfmt clippy
  rustup toolchain install nightly --component rustfmt
}

install_risc0() {
  if ! have rzup; then
    log "Installing rzup (RISC Zero toolchain manager)..."
    curl -L https://risczero.com/install | bash
    export PATH="$HOME/.risc0/bin:$PATH"
  fi
  log "Installing the RISC Zero zkVM toolchain via rzup..."
  rzup install
}

install_solana() {
  if ! have solana; then
    log "Installing the Agave (Solana) CLI (${SOLANA_VERSION})..."
    sh -c "$(curl -sSfL "https://release.anza.xyz/${SOLANA_VERSION}/install")"
    export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
  else
    log "Solana CLI already present; skipping (re-run in a fresh shell to upgrade)."
  fi
}

install_anchor() {
  ensure_rustup
  if ! have avm; then
    log "Installing avm (Anchor version manager)..."
    cargo install --git https://github.com/coral-xyz/anchor avm --force
  fi
  log "Installing and selecting Anchor ${ANCHOR_VERSION}..."
  avm install "${ANCHOR_VERSION}"
  avm use "${ANCHOR_VERSION}"
}

verify() {
  log "Installed toolchain versions:"
  for tool in "rustc --version" "cargo --version" "rzup --version" \
              "r0vm --version" "cargo-risczero --version" "solana --version" \
              "avm --version" "anchor --version"; do
    name="${tool%% *}"
    if have "$name"; then
      printf '  %-16s %s\n' "$name" "$($tool 2>/dev/null | head -1)"
    else
      printf '  %-16s %s\n' "$name" "NOT INSTALLED"
    fi
  done
}

usage() {
  sed -n '2,29p' "$0" | sed 's/^# \{0,1\}//'
}

# --- Argument dispatch --------------------------------------------------------
main() {
  if [[ $# -eq 0 ]]; then set -- --all; fi

  local do_rust=0 do_risc0=0 do_solana=0 do_anchor=0 do_verify=0
  for arg in "$@"; do
    case "$arg" in
      --all)    do_rust=1; do_risc0=1; do_solana=1; do_anchor=1 ;;
      --rust)   do_rust=1 ;;
      --risc0)  do_risc0=1 ;;
      --solana) do_solana=1 ;;
      --anchor) do_anchor=1 ;;
      --verify) do_verify=1 ;;
      -h|--help) usage; exit 0 ;;
      *) warn "unknown option: $arg"; usage; exit 1 ;;
    esac
  done

  if [[ $do_verify -eq 1 ]]; then verify; exit 0; fi

  [[ $do_rust   -eq 1 ]] && install_rust
  [[ $do_risc0  -eq 1 ]] && install_risc0
  [[ $do_solana -eq 1 ]] && install_solana
  [[ $do_anchor -eq 1 ]] && install_anchor

  echo ""
  verify
  echo ""
  log "Done. Open a new shell so all PATH updates take effect."
}

main "$@"
