#!/usr/bin/env bash
#
# with-webdav.sh — strict sccache storage-chain wrapper for CI.
#
# Owns exactly one decision and nothing else: which sccache storage chain the
# gate runs under. It never edits Cargo configuration, never touches the repo,
# and never decides product behavior.
#
# Contract
# --------
# The build box (Forgejo runner) injects cache-scoped WebDAV credentials as
# BUILDER_WEBDAV_* environment variables. They arrive in this process's
# environment and are treated as secret:
#
#   * never printed (stdout or stderr),
#   * never placed in argv or in a URL,
#   * never written to a file, a persistent git config, or an env that leaks
#     into a subprocess other than the mapped SCCACHE_WEBDAV_* below.
#
# Required for a WebDAV L1:
#   BUILDER_WEBDAV_ENDPOINT, plus either
#     BUILDER_WEBDAV_TOKEN                         (bearer auth), or
#     BUILDER_WEBDAV_USERNAME + BUILDER_WEBDAV_PASSWORD  (basic auth).
# Optional:
#   BUILDER_WEBDAV_KEY_PREFIX   -> mapped to SCCACHE_WEBDAV_KEY_PREFIX.
#
# Behaviour
# ---------
#   * If the full WebDAV configuration is present, an authenticated
#     PROPFIND (Depth: 0) is issued against the endpoint with a 5s timeout.
#     On a 2xx response the chain is `disk,webdav` (L0 = local disk,
#     L1 = shared WebDAV). On any other outcome — missing config, auth
#     failure, timeout, connection refused — the chain is `disk`.
#   * SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY is always `l0`: a failed L1 write
#     never fails a compile; only a failed L0 (disk) write does.
#   * SCCACHE_CACHE_SIZE is forced to 6G in CI only. The repo's workstation
#     default (40G in .cargo/config.toml, force=false) is unchanged because an
#     environment variable always wins over that setting.
#   * The supplied command is exec'd with the selected chain in effect.
#
# Credential hygiene
# ------------------
# The health check uses curl's `-K -` (config read from stdin): the
# Authorization header is fed to curl over a pipe, never via `curl --user`,
# which would place `user:password` in the process argv.
#
# Usage
# -----
#   with-webdav.sh [--print-chain] [--] COMMAND [ARG ...]
#
#   --print-chain   print only the selected chain (`disk` or `disk,webdav`)
#                   to stdout and exit 0 without running COMMAND. Used by the
#                   proof harness and by targeted shell tests.

set -euo pipefail

print_chain=0
case "${1:-}" in
  --print-chain)
    print_chain=1
    shift
    ;;
esac
case "${1:-}" in
  --)
    shift
    ;;
esac

if [[ "$print_chain" -eq 0 && "$#" -eq 0 ]]; then
  echo "with-webdav.sh: no command supplied" >&2
  echo "usage: with-webdav.sh [--print-chain] [--] COMMAND [ARG ...]" >&2
  exit 2
fi

# Start from a clean slate: the mapping below is the only source of
# SCCACHE_WEBDAV_* in this process tree. This keeps a disk-only selection from
# accidentally inheriting stale WebDAV configuration.
unset SCCACHE_WEBDAV_ENDPOINT SCCACHE_WEBDAV_USERNAME SCCACHE_WEBDAV_PASSWORD \
      SCCACHE_WEBDAV_TOKEN SCCACHE_WEBDAV_KEY_PREFIX

# Forgejo-safe CI cache budget. Does not modify .cargo/config.toml.
export SCCACHE_CACHE_SIZE=6G

# Multi-level write policy: L0 (disk) is the only level whose write failure
# fails a compile. L1 (WebDAV) is best-effort.
export SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY=l0

# True iff the build box injected a complete WebDAV configuration.
webdav_config_complete() {
  [[ -n "${BUILDER_WEBDAV_ENDPOINT:-}" ]] || return 1
  if [[ -n "${BUILDER_WEBDAV_TOKEN:-}" ]]; then
    return 0
  fi
  [[ -n "${BUILDER_WEBDAV_USERNAME:-}" && -n "${BUILDER_WEBDAV_PASSWORD:-}" ]]
}

# Emit the Authorization header value on stdout. Credential material stays in
# shell memory only. Callers capture the output; this is never echoed.
webdav_authorization_value() {
  if [[ -n "${BUILDER_WEBDAV_TOKEN:-}" ]]; then
    printf 'Authorization: Bearer %s' "$BUILDER_WEBDAV_TOKEN"
  else
    local basic
    basic="$(printf '%s:%s' "${BUILDER_WEBDAV_USERNAME}" "${BUILDER_WEBDAV_PASSWORD}" \
      | base64 | tr -d '\n')"
    printf 'Authorization: Basic %s' "$basic"
  fi
}

# Issue an authenticated PROPFIND (Depth: 0) and report 2xx as healthy.
# A timeout or refused connection yields status 000 and a non-zero curl exit;
# both are treated as unhealthy (never as a wrapper failure).
webdav_healthy() {
  local auth status
  auth="$(webdav_authorization_value)"
  if status="$(curl -K - -sS -o /dev/null -w '%{http_code}' --max-time 5 \
       -X PROPFIND -H 'Depth: 0' "${BUILDER_WEBDAV_ENDPOINT}" 2>/dev/null <<EOF
header = "${auth}"
EOF
  )"; then
    :
  else
    status="${status:-000}"
  fi
  [[ "$status" =~ ^2[0-9][0-9]$ ]]
}

chain=disk
if webdav_config_complete; then
  if webdav_healthy; then
    chain=disk,webdav
    export SCCACHE_WEBDAV_ENDPOINT="$BUILDER_WEBDAV_ENDPOINT"
    if [[ -n "${BUILDER_WEBDAV_TOKEN:-}" ]]; then
      export SCCACHE_WEBDAV_TOKEN="$BUILDER_WEBDAV_TOKEN"
    else
      export SCCACHE_WEBDAV_USERNAME="$BUILDER_WEBDAV_USERNAME"
      export SCCACHE_WEBDAV_PASSWORD="$BUILDER_WEBDAV_PASSWORD"
    fi
    if [[ -n "${BUILDER_WEBDAV_KEY_PREFIX:-}" ]]; then
      export SCCACHE_WEBDAV_KEY_PREFIX="$BUILDER_WEBDAV_KEY_PREFIX"
    fi
  else
    echo "with-webdav: WebDAV health check failed; using disk-only cache" >&2
  fi
fi

export SCCACHE_MULTILEVEL_CHAIN="$chain"

if [[ "$print_chain" -eq 1 ]]; then
  printf '%s\n' "$chain"
  exit 0
fi

exec "$@"
