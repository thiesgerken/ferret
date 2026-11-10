#!/usr/bin/env python3
"""Refresh the official pg_search binary pins after a pg_search release update."""

import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def asset_hash(repository, version, asset):
    url = f"https://github.com/{repository}/releases/download/v{version}/{asset}"
    result = subprocess.check_output(
        ["nix", "store", "prefetch-file", "--json", url], text=True
    )
    return json.loads(result)["hash"]


def refresh_pg_search():
    path = ROOT / "nix/pg-search-binaries.json"
    previous = path.read_text()
    pins = json.loads(previous)
    for system, architecture in {"aarch64-linux": "arm64", "x86_64-linux": "amd64"}.items():
        name = f"postgresql-18-pg-search_{pins['version']}-1PARADEDB-bookworm_{architecture}.deb"
        pins["platforms"][system] = {
            "asset": name,
            "hash": asset_hash("paradedb/paradedb", pins["version"], name),
        }
    updated = json.dumps(pins, indent=2) + "\n"
    if updated != previous:
        path.write_text(updated)
        print("Updated the pg_search binary pins. Commit them with the version change.")


if __name__ == "__main__":
    refresh_pg_search()
