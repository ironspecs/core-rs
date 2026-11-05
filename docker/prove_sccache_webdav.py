#!/usr/bin/env python3
"""
prove_sccache_webdav.py — isolated proof harness for the sccache L0->L1 WebDAV
pilot.

Owns nothing in the repo. It runs real `cargo build --locked -p collections`
builds inside the builder container and verifies, from sccache's JSON stats,
that the wrapper plus multi-level cache behave as designed:

  1. Negative control — with no BUILDER_WEBDAV_* variables, the wrapper
     selects a disk-only chain and the crate still compiles.
  2. Closed-port control — with BUILDER_WEBDAV_* present but the endpoint on a
     closed port, the wrapper's health check fails, it still selects a
     disk-only chain, and the crate still compiles.
  3. Healthy proof — when the build box's WebDAV is reachable, the wrapper
     selects `disk,webdav` and the multi-level cache behaves as documented:
       * initial build (warm or cold),
       * clear ONLY the isolated L0 directory and the isolated target dir,
       * a second identical build must increase L1 hits and L0 backfill
         WITHOUT increasing compilations,
       * a third build (target cleared again) must show an L0 hit.

Isolation
---------
Every phase uses its own SCCACHE_DIR (isolated L0), CARGO_TARGET_DIR, and
SCCACHE_SERVER_PORT, so it can neither observe nor disturb the gate's caches or
the gate's sccache daemon. Async L1->L0 backfill is settled with a bounded
poll. The initial build's async L1 write is awaited with a bounded
remote-readiness poll before L0 is cleared, so build2 cannot race it. Cargo
"Fresh" output is never accepted as evidence: every asserted phase
must increase sccache `compile_requests`, which only happens when rustc is
actually re-invoked.

Hygiene
-------
Prints counts only. It never prints BUILDER_WEBDAV_* values. It cleans up only
the artifacts it created (temp directories and its sccache servers). It never
modifies a repo file and never deletes anything from the shared WebDAV L1.

Exit status: 0 on success (including when the live WebDAV proof is skipped
because the endpoint is unhealthy), 1 on any assertion or execution failure.
"""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

COLLECTIONS = ["cargo", "build", "--locked", "-p", "collections"]
SETTLE_TIMEOUT_SECONDS = 20.0
SETTLE_INTERVAL_SECONDS = 0.25


class ProofError(RuntimeError):
    """A proof assertion or execution failure."""


def repo_root() -> Path:
    """Repository checkout directory (parent of docker/)."""
    return Path(__file__).resolve().parents[1]


def free_port() -> int:
    """Return a localhost TCP port that is currently free (bind then close)."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])
    finally:
        sock.close()


def run(args, *, env, cwd: Path, capture: bool = True) -> subprocess.CompletedProcess:
    """Run a command, capturing (or not) its stdout/stderr as text."""
    return subprocess.run(
        list(args),
        env=env,
        cwd=str(cwd),
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )


def show_stats(env) -> dict:
    """Fetch sccache JSON stats from the server selected by SCCACHE_SERVER_PORT."""
    proc = run(["sccache", "--show-stats", "--stats-format=json"], env=env, cwd=repo_root())
    if proc.returncode != 0:
        raise ProofError(
            f"sccache --show-stats failed (rc={proc.returncode}): {proc.stderr.strip()}"
        )
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise ProofError(f"sccache --show-stats returned invalid JSON: {exc}") from exc


def level(stats: dict, idx: int) -> dict:
    """Return the multi_level stats entry for the given cache level index."""
    ml = stats["stats"].get("multi_level")
    if not ml or idx >= len(ml):
        raise ProofError(f"multi_level[{idx}] not present in stats")
    return ml[idx]


def compilations(stats: dict) -> int:
    return int(stats["stats"]["compilations"])


def compile_requests(stats: dict) -> int:
    return int(stats["stats"]["compile_requests"])


def l0_hits(stats: dict) -> int:
    return int(level(stats, 0)["hits"])


def l0_backfills_to(stats: dict) -> int:
    return int(level(stats, 0)["backfills_to"])


def l1_hits(stats: dict) -> int:
    return int(level(stats, 1)["hits"])


def l1_writes(stats: dict) -> int:
    return int(level(stats, 1)["writes"])


def wrapper_chain(wrapper: Path, env, cwd: Path) -> str:
    """Ask the wrapper which chain it would select (disk or disk,webdav)."""
    proc = run([str(wrapper), "--print-chain"], env=env, cwd=cwd)
    if proc.returncode != 0:
        raise ProofError(
            f"with-webdav.sh --print-chain failed (rc={proc.returncode}): {proc.stderr.strip()}"
        )
    return proc.stdout.strip()


def cargo_build(wrapper: Path, env, cwd: Path) -> None:
    """Run a real build through the wrapper (so the wrapper selects the chain)."""
    proc = run([str(wrapper), "--", *COLLECTIONS], env=env, cwd=cwd)
    if proc.returncode != 0:
        tail = (proc.stderr or proc.stdout or "")[-4000:]
        raise ProofError(f"cargo build failed (rc={proc.returncode}):\n{tail}")


def stop_server(env) -> None:
    """Best-effort shutdown of the sccache daemon on this phase's port."""
    run(["sccache", "--stop-server"], env=env, cwd=repo_root())


def clear_dir_contents(path: str) -> None:
    """Remove every entry inside `path`, keeping the directory itself."""
    for entry in os.listdir(path):
        entry_path = os.path.join(path, entry)
        if os.path.isdir(entry_path) and not os.path.islink(entry_path):
            shutil.rmtree(entry_path)
        else:
            os.remove(entry_path)


def settle_backfill(env) -> dict:
    """Poll until L0 backfill stops increasing (bounded async settling)."""
    deadline = time.monotonic() + SETTLE_TIMEOUT_SECONDS
    prev = -1
    last = None
    while time.monotonic() < deadline:
        stats = show_stats(env)
        last = stats
        count = l0_backfills_to(stats)
        if count == prev and count > 0:
            return stats
        prev = count
        time.sleep(SETTLE_INTERVAL_SECONDS)
    # Timeout reached; return the latest snapshot and let the assertions judge
    # whether enough backfill landed.
    return last if last is not None else show_stats(env)


def wait_remote_ready(env) -> dict:
    """Poll until the initial build's L1 write has landed (bounded settling).

    The first build's L1 write is async: build2 would otherwise race it, and
    clearing L0 before that write lands orphans the second build into a full
    recompile. Ready means either L1 already hit (> 0, warm start) or the
    initial L1 write has landed (writes > 0). Raises on timeout with the
    counts only.
    """
    deadline = time.monotonic() + SETTLE_TIMEOUT_SECONDS
    last = None
    while time.monotonic() < deadline:
        stats = show_stats(env)
        last = stats
        if l1_hits(stats) > 0 or l1_writes(stats) > 0:
            return stats
        time.sleep(SETTLE_INTERVAL_SECONDS)
    hits = l1_hits(last) if last is not None else 0
    writes = l1_writes(last) if last is not None else 0
    raise ProofError(
        f"L1 not ready after {SETTLE_TIMEOUT_SECONDS:g}s "
        f"(l1_hits={hits}, l1_writes={writes})"
    )


def without_builder(env: dict) -> dict:
    """Return a copy of `env` with all BUILDER_WEBDAV_* removed."""
    return {k: v for k, v in env.items() if not k.startswith("BUILDER_WEBDAV_")}


class Isolation:
    """Tracks temp dirs and ports so cleanup only touches what we created."""

    def __init__(self, base_env: dict):
        self.base_env = dict(base_env)
        self.temp_dirs: list[str] = []
        self.ports: list[int] = []

    def env(self, source: dict, extra: dict | None = None) -> dict:
        env = dict(source)
        if extra:
            env.update(extra)
        sccache_dir = tempfile.mkdtemp(prefix="sccache-proof-l0-")
        target_dir = tempfile.mkdtemp(prefix="sccache-proof-target-")
        port = free_port()
        self.temp_dirs.extend((sccache_dir, target_dir))
        self.ports.append(port)
        env["SCCACHE_DIR"] = sccache_dir
        env["CARGO_TARGET_DIR"] = target_dir
        env["SCCACHE_SERVER_PORT"] = str(port)
        env["CARGO_INCREMENTAL"] = "0"
        return env

    def cleanup(self) -> None:
        for port in self.ports:
            stop_env = dict(self.base_env)
            stop_env["SCCACHE_SERVER_PORT"] = str(port)
            try:
                stop_server(stop_env)
            except Exception:
                pass
        for directory in self.temp_dirs:
            shutil.rmtree(directory, ignore_errors=True)


def negative_control(wrapper: Path, checkout: Path, iso: Isolation) -> None:
    env = iso.env(without_builder(iso.base_env))
    chain = wrapper_chain(wrapper, env, checkout)
    if chain != "disk":
        raise ProofError(f"negative control: expected chain 'disk', got '{chain}'")
    cargo_build(wrapper, env, checkout)
    comp = compilations(show_stats(env))
    if comp == 0:
        raise ProofError("negative control: build produced 0 compilations (Cargo Fresh?)")
    print(f"proof: negative control chain={chain} compilations={comp}")


def closed_port_control(wrapper: Path, checkout: Path, iso: Isolation) -> None:
    closed_port = free_port()
    env = iso.env(
        without_builder(iso.base_env),
        extra={
            "BUILDER_WEBDAV_ENDPOINT": f"http://127.0.0.1:{closed_port}/webdav",
            "BUILDER_WEBDAV_USERNAME": "dummy",
            "BUILDER_WEBDAV_PASSWORD": "dummy-password",
        },
    )
    chain = wrapper_chain(wrapper, env, checkout)
    if chain != "disk":
        raise ProofError(f"closed-port control: expected chain 'disk', got '{chain}'")
    cargo_build(wrapper, env, checkout)
    comp = compilations(show_stats(env))
    if comp == 0:
        raise ProofError("closed-port control: build produced 0 compilations (Cargo Fresh?)")
    print(f"proof: closed-port control chain={chain} compilations={comp}")


def healthy_proof(wrapper: Path, checkout: Path, iso: Isolation) -> None:
    # The live proof requires a reachable WebDAV. When it is down, skip rather
    # than fail: that is exactly the outage the wrapper is designed to absorb.
    chain = wrapper_chain(wrapper, iso.base_env, checkout)
    if chain != "disk,webdav":
        print(f"proof: L0->L1 skipped (WebDAV unhealthy, chain={chain})")
        return

    env = iso.env(iso.base_env)  # keeps the inherited BUILDER_WEBDAV_*

    cargo_build(wrapper, env, checkout)
    s1 = show_stats(env)

    # Wait for build1's async L1 write to land before clearing L0; otherwise
    # build2 races it and falls back to a full recompile.
    wait_remote_ready(env)

    clear_dir_contents(env["SCCACHE_DIR"])
    clear_dir_contents(env["CARGO_TARGET_DIR"])

    cargo_build(wrapper, env, checkout)
    s2 = settle_backfill(env)

    clear_dir_contents(env["CARGO_TARGET_DIR"])

    cargo_build(wrapper, env, checkout)
    s3 = show_stats(env)

    c1, c2, c3 = compilations(s1), compilations(s2), compilations(s3)
    r1, r2, r3 = compile_requests(s1), compile_requests(s2), compile_requests(s3)
    l1h1, l1h2 = l1_hits(s1), l1_hits(s2)
    bf1, bf2 = l0_backfills_to(s1), l0_backfills_to(s2)
    l0h2, l0h3 = l0_hits(s2), l0_hits(s3)

    if r2 <= r1:
        raise ProofError("build2 issued no new compile requests (Cargo Fresh?)")
    if c2 > c1:
        raise ProofError(f"build2 increased compilations ({c1} -> {c2})")
    if l1h2 <= l1h1:
        raise ProofError(f"build2 did not increase L1 hits ({l1h1} -> {l1h2})")
    if bf2 <= bf1:
        raise ProofError(f"build2 did not increase L0 backfill ({bf1} -> {bf2})")
    if r3 <= r2:
        raise ProofError("build3 issued no new compile requests (Cargo Fresh?)")
    if l0h3 <= l0h2:
        raise ProofError(f"build3 did not show an L0 hit ({l0h2} -> {l0h3})")

    print(
        f"proof: L0->L1 build1 compilations={c1} l1_hits={l1h1} l0_backfills_to={bf1}"
    )
    print(
        f"proof: L0->L1 build2 compilations={c2} (+{c2 - c1}) "
        f"l1_hits={l1h2} (+{l1h2 - l1h1}) l0_backfills_to={bf2} (+{bf2 - bf1})"
    )
    print(f"proof: L0->L1 build3 l0_hits={l0h3} (+{l0h3 - l0h2})")


def main() -> int:
    checkout = repo_root()
    wrapper = checkout / "docker" / "with-webdav.sh"
    if not wrapper.is_file():
        print(f"proof: wrapper not found at {wrapper}", file=sys.stderr)
        return 1

    iso = Isolation(os.environ)
    try:
        print("proof: collections sccache L0->L1 pilot")
        negative_control(wrapper, checkout, iso)
        closed_port_control(wrapper, checkout, iso)
        healthy_proof(wrapper, checkout, iso)
        print("proof: ok")
        return 0
    except ProofError as exc:
        print(f"proof: FAILED: {exc}", file=sys.stderr)
        return 1
    finally:
        iso.cleanup()


if __name__ == "__main__":
    sys.exit(main())
