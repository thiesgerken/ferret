#!/usr/bin/env python3
"""Exercise locally loaded images in an isolated, disposable Compose project."""

import json
from pathlib import Path
import re
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parent.parent


def run(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def request(url, payload=None, cookie=None):
    headers = {}
    if payload is not None:
        headers["Content-Type"] = "application/json"
    if cookie:
        headers["Cookie"] = cookie
    req = urllib.request.Request(
        url, data=json.dumps(payload).encode() if payload is not None else None, headers=headers
    )
    return urllib.request.urlopen(req, timeout=15)


def eventually(operation, timeout=120):
    deadline = time.monotonic() + timeout
    while True:
        try:
            return operation()
        except (AssertionError, urllib.error.URLError, TimeoutError):
            if time.monotonic() >= deadline:
                raise
            time.sleep(1)


def main():
    architecture = run("docker", "info", "--format", "{{.Architecture}}")
    arm = architecture in ("aarch64", "arm64")
    assert arm or architecture in ("x86_64", "amd64"), architecture
    expected = "arm64" if arm else "amd64"
    for image in ("ferret", "ferret-plugins", "ferret-frontend"):
        actual = run("docker", "image", "inspect", f"{image}:local", "--format", "{{.Architecture}}")
        assert actual == expected, (image, actual, expected)

    with tempfile.TemporaryDirectory(prefix="ferret-smoke-") as state:
        state = Path(state)
        # Docker Desktop must be able to traverse the bind mount as the image's non-root user.
        state.chmod(0o755)
        repo = state / "repo"
        repo.mkdir()
        run("git", "init", "--quiet", str(repo))
        (repo / "smoke.md").write_text("# Runtime smoke\n\nferretsmokevalidation searches a real Git document.\n")
        run("git", "-C", str(repo), "add", "smoke.md")
        run("git", "-C", str(repo), "-c", "user.name=Ferret Smoke", "-c", "user.email=smoke@example.invalid",
            "commit", "--quiet", "-m", "Runtime fixture")
        overlay = state / "compose.json"
        overlay.write_text(json.dumps({"services": {
            "server": {"image": "ferret:local", "pull_policy": "never", "ports": ["127.0.0.1::8080"]},
            "plugins": {"image": "ferret-plugins:local", "pull_policy": "never",
                        "volumes": [f"{repo}:/smoke/repo:ro"], "environment": {
                            "FERRET_GIT_PROTOCOLS": "file", "GIT_CONFIG_COUNT": "1",
                            "GIT_CONFIG_KEY_0": "safe.directory", "GIT_CONFIG_VALUE_0": "/smoke/repo"}},
            "frontend": {"image": "ferret-frontend:local", "pull_policy": "never"},
            "proxy": {"ports": ["127.0.0.1::80"]},
        }}))
        # Compose's explicit reset tags prevent the base file's fixed host ports from leaking in.
        reset = state / "reset.yaml"
        reset.write_text("services:\n  server:\n    ports: !reset []\n  proxy:\n    ports: !reset []\n  paradedb:\n    ports: !reset []\n")
        compose = ["docker", "compose", "--project-directory", str(ROOT), "-p", f"ferret-smoke-{uuid.uuid4().hex[:10]}",
                   "-f", str(ROOT / "docker-compose.yml")]
        if arm:
            compose += ["-f", str(ROOT / "docker-compose.arm.yml")]
        compose += ["-f", str(reset), "-f", str(overlay)]
        try:
            run(*compose, "up", "-d", "paradedb", "server", "plugins", "frontend", "proxy")
            server = "http://" + run(*compose, "port", "server", "8080")
            frontend = "http://" + run(*compose, "port", "proxy", "80")
            eventually(lambda: request(server + "/healthz").read())
            login = eventually(lambda: request(frontend + "/auth/login", {
                "username": "admin", "password": "ferret-development"
            }))
            cookie = login.headers["Set-Cookie"].split(";", 1)[0]

            def graphql(query, variables=None):
                result = json.load(request(frontend + "/graphql", {"query": query, "variables": variables or {}}, cookie))
                assert not result.get("errors"), result
                return result["data"]

            def worker_ready():
                types = graphql("{ pluginTypes { name } }")["pluginTypes"]
                assert any(plugin["name"] == "git" for plugin in types), types

            eventually(worker_ready)
            graphql("mutation($input: NewConnection!) { createConnection(input: $input) { id } }", {"input": {
                "name": "Runtime Git smoke", "icon": "git-branch", "plugin": "git", "source": "runtime-smoke",
                "config": json.dumps({"remote": "file:///smoke/repo", "include": ["*.md"],
                                      "url_template": "https://example.invalid/{path}"}), "schedule": "0 * * * *"
            }})

            def git_ingested():
                connection = graphql("{ connections { runs(limit: 1) { status error } } }")["connections"][0]
                assert connection["runs"], connection
                latest = connection["runs"][0]
                if latest["status"] in ("FAILED", "LOST"):
                    raise RuntimeError(latest)
                assert latest["status"] == "SUCCEEDED", latest
                result = graphql('{ search(input: {q: "ferretsmokevalidation", mode: KEYWORD}) { total } }')
                assert result["search"]["total"] == 1, result

            eventually(git_ingested)
            for path in ("/", "/connections", "/settings"):
                response = request(frontend + path)
                assert response.status == 200, path
                html = response.read().decode()
                assert "<html" in html, path
            response = request(frontend + "/")
            assert "no-store" in response.headers["Cache-Control"]
            html = response.read().decode()
            asset = re.search(r'(/_next/static/[^" ]+\.js)', html)
            assert asset, "No static JavaScript asset in frontend export"
            response = request(frontend + asset.group(1))
            assert "immutable" in response.headers["Cache-Control"]
            try:
                request(frontend + "/not-a-ferret-page")
                raise AssertionError("Unknown frontend route did not return 404")
            except urllib.error.HTTPError as error:
                assert error.code == 404, error
            for service in ("server", "plugins", "frontend"):
                container = run(*compose, "ps", "-q", service)
                config = json.loads(run("docker", "inspect", container))[0]
                assert config["HostConfig"]["ReadonlyRootfs"], service
                assert config["Config"]["User"] == "65532:65532", service
                assert config["State"]["Running"], service
            # This runs the worker image's real Git HTTPS transport and system certificate bundle.
            run("docker", "run", "--rm", "--read-only", "--cap-drop=ALL", "--security-opt=no-new-privileges",
                "--tmpfs", "/tmp:rw,nosuid,nodev,mode=1777", "--entrypoint", "git", "ferret-plugins:local",
                "ls-remote", "https://github.com/thiesgerken/ferret.git", "HEAD")
            print(f"PASS {expected}: database health/migrations, login, worker Git ingestion/search, TLS Git, frontend routes/cache, non-root read-only runtime")
        except BaseException:
            subprocess.run([*compose, "logs", "--no-color"], check=False)
            raise
        finally:
            subprocess.run([*compose, "down", "--volumes", "--remove-orphans"], check=True)


if __name__ == "__main__":
    main()
