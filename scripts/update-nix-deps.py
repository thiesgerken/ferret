#!/usr/bin/env python3
"""Refresh official binary pins and fetchPnpmDeps after dependency updates."""

import argparse
import base64
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def release_assets(repository, version):
    release = json.loads(subprocess.check_output(
        ["gh", "api", f"repos/{repository}/releases/tags/v{version}"], text=True
    ))
    return {asset["name"]: asset for asset in release["assets"]}


def asset_hash(asset):
    digest = asset["digest"]
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
        raise ValueError(f"Missing official SHA-256 for {asset['name']}")
    return "sha256-" + base64.b64encode(bytes.fromhex(digest[7:])).decode()


def refresh_pg_search():
    path = ROOT / "nix/pg-search-binaries.json"
    previous = path.read_text()
    pins = json.loads(previous)
    assets = release_assets("paradedb/paradedb", pins["version"])
    for system, architecture in {"aarch64-linux": "arm64", "x86_64-linux": "amd64"}.items():
        name = f"postgresql-18-pg-search_{pins['version']}-1PARADEDB-bookworm_{architecture}.deb"
        pins["platforms"][system] = {"asset": name, "hash": asset_hash(assets[name])}
    updated = json.dumps(pins, indent=2) + "\n"
    if updated != previous:
        path.write_text(updated)
    print("Refreshed official pg_search binary pins. Run all required Linux checks and image smokes.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--system", choices=("x86_64-linux", "aarch64-linux"))
    mode.add_argument("--pg-search", action="store_true", help="Refresh only the selected pg_search release's binary pins")
    args = parser.parse_args()
    if args.pg_search:
        refresh_pg_search()
        return
    manager = json.loads((ROOT / "frontend/package.json").read_text())["packageManager"]
    match = re.fullmatch(r"pnpm@([0-9]+\.[0-9]+\.[0-9]+)", manager)
    if not match:
        raise ValueError(f"Expected an exact pnpm packageManager, got {manager!r}")
    pins_path = ROOT / "nix/pnpm-binaries.json"
    pins = json.loads(pins_path.read_text())
    if pins["version"] != match[1]:
        assets = release_assets("pnpm/pnpm", match[1])
        for platform in pins["platforms"].values():
            platform["hash"] = asset_hash(assets[platform["asset"]])
        pins["version"] = match[1]
        pins_path.write_text(json.dumps(pins, indent=2) + "\n")

    hash_file = ROOT / "nix/pnpm-deps-hash"
    previous_hash = hash_file.read_text()
    hash_file.write_text("")
    command = ["nix", "build", f".#packages.{args.system}.pnpmDeps", "--no-link", "--print-build-logs"]
    try:
        hashes = []
        with subprocess.Popen(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
            assert process.stdout is not None
            for line in process.stdout:
                print(line, end="", flush=True)
                hashes.extend(re.findall(r"got:\s+(sha256-[A-Za-z0-9+/]+=*)", line))
            returncode = process.wait()
        if returncode == 0 or len(hashes) != 1:
            raise RuntimeError("Expected one fetchPnpmDeps hash mismatch; resolve the actual build failure first")
        hash_file.write_text(hashes[0] + "\n")
        subprocess.run(command, cwd=ROOT, check=True)
    except BaseException:
        hash_file.write_text(previous_hash)
        raise
    print("Updated official pnpm binary pins and dependency hash. Commit them with the dependency change, then run all required checks/builds/smokes.")


if __name__ == "__main__":
    main()
