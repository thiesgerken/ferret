#!/usr/bin/env python3
"""Refresh official pnpm binary pins and fetchPnpmDeps hash after dependency updates."""

import argparse
import base64
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--system", choices=("x86_64-linux", "aarch64-linux"), required=True)
    args = parser.parse_args()
    manager = json.loads((ROOT / "frontend/package.json").read_text())["packageManager"]
    match = re.fullmatch(r"pnpm@([0-9]+\.[0-9]+\.[0-9]+)", manager)
    if not match:
        raise ValueError(f"Expected an exact pnpm packageManager, got {manager!r}")
    pins_path = ROOT / "nix/pnpm-binaries.json"
    pins = json.loads(pins_path.read_text())
    if pins["version"] != match[1]:
        release = json.loads(subprocess.check_output(
            ["gh", "api", f"repos/pnpm/pnpm/releases/tags/v{match[1]}"], text=True
        ))
        assets = {asset["name"]: asset for asset in release["assets"]}
        for platform in pins["platforms"].values():
            digest = assets[platform["asset"]]["digest"]
            if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
                raise ValueError(f"Missing official SHA-256 for {platform['asset']}")
            platform["hash"] = "sha256-" + base64.b64encode(bytes.fromhex(digest[7:])).decode()
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
