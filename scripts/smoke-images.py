#!/usr/bin/env python3
"""Exercise locally loaded images in an isolated, disposable Compose project."""

from contextlib import ExitStack
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import secrets
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parent.parent


class FontPreloads(HTMLParser):
    def __init__(self):
        super().__init__()
        self.hrefs = set()

    def handle_starttag(self, tag, attrs):
        attributes = dict(attrs)
        if tag == "link" and attributes.get("as") == "font":
            self.hrefs.add(attributes["href"])


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


def smoke_whatsapp_archive(state, architecture):
    archive_dir = state / "archive"
    archive_dir.mkdir(mode=0o755)
    database = archive_dir / "wacli.db"
    token = secrets.token_hex(32)
    token_file = state / "archive-token"
    token_file.write_text(token + "\n")
    token_file.chmod(0o644)
    name = f"ferret-archive-smoke-{uuid.uuid4().hex[:10]}"
    with ExitStack() as cleanup:
        writer = sqlite3.connect(database)
        cleanup.callback(writer.close)
        assert writer.execute("PRAGMA journal_mode=WAL").fetchone()[0] == "wal"
        writer.execute("PRAGMA wal_autocheckpoint=0")
        fixture = ROOT / "crates/ferret-whatsapp-archive/tests/fixtures/wacli-v0.22.0.sql"
        writer.executescript(fixture.read_text())
        writer.execute("INSERT INTO chats(jid,kind,name) VALUES('1@s.whatsapp.net','dm','Offline fixture')")
        writer.execute(
            "INSERT INTO messages(chat_jid,msg_id,ts,from_me,text,direct_path,media_key,local_path) "
            "VALUES('1@s.whatsapp.net','first',1,0,'Offline archive smoke','privatepath',x'010203','/private/media')"
        )
        writer.commit()
        for suffix in ("", "-wal", "-shm"):
            path = Path(str(database) + suffix)
            assert path.stat().st_size > 0, path
            path.chmod(0o644)
        run("docker", "network", "create", "--internal", name)
        cleanup.callback(run, "docker", "network", "rm", name)
        assert run("docker", "network", "inspect", name, "--format", "{{.Internal}}") == "true"
        container = run(
            "docker", "run", "-d", "--name", name, "--network", name,
            "--platform", f"linux/{architecture}", "--read-only", "--cap-drop=ALL",
            "--security-opt=no-new-privileges", "--tmpfs", "/tmp:rw,nosuid,nodev,mode=1777",
            "--mount", f"type=bind,src={archive_dir},dst=/archive,readonly",
            "--mount", f"type=bind,src={token_file},dst=/run/secrets/archive-token,readonly",
            "-e", "WHATSAPP_ARCHIVE_DB=/archive/wacli.db",
            "-e", "WHATSAPP_ARCHIVE_TOKEN_FILE=/run/secrets/archive-token",
            "-p", "127.0.0.1::8080", "ferret-whatsapp-archive:local"
        )
        cleanup.callback(run, "docker", "rm", "--force", container)
        try:
            base = "http://" + run("docker", "port", container, "8080/tcp")
            eventually(lambda: request(base + "/healthz").read())
            for route in ("archive", "chats", "messages", "changes"):
                try:
                    request(base + "/v1/" + route)
                    raise AssertionError(f"Unauthenticated archive {route} did not return 401")
                except urllib.error.HTTPError as error:
                    assert error.code == 401, error
                    assert json.load(error) == {"code": "unauthorized"}

            def archive_get(route):
                return urllib.request.urlopen(urllib.request.Request(
                    base + route, headers={"Authorization": "Bearer " + token}
                ), timeout=15)

            archive = json.load(archive_get("/v1/archive"))
            assert archive["schema_version"] == 32, archive
            assert archive["collector_status"] == "unknown", archive
            store_id = archive["store_id"]
            query = urllib.parse.urlencode({"store_id": store_id, "chat": "1@s.whatsapp.net"})

            def verify_messages(texts):
                response = archive_get("/v1/messages?" + query).read().decode()
                messages = json.loads(response)["messages"]
                assert [message["text"] for message in messages] == texts, messages
                for forbidden in ("privatepath", "media_key", "direct_path", "local_path", "/private/media"):
                    assert forbidden not in response, forbidden

            verify_messages(["Offline archive smoke"])
            writer.execute(
                "INSERT INTO messages(chat_jid,msg_id,ts,from_me,text) "
                "VALUES('1@s.whatsapp.net','second',2,0,'Live WAL follow-up')"
            )
            writer.commit()
            verify_messages(["Offline archive smoke", "Live WAL follow-up"])
            writer.close()
            for suffix in ("-wal", "-shm"):
                assert Path(str(database) + suffix).is_file(), suffix
            verify_messages(["Offline archive smoke", "Live WAL follow-up"])
            run("docker", "restart", container)
            eventually(lambda: request(base + "/healthz").read())
            assert json.load(archive_get("/v1/archive"))["store_id"] == store_id
            verify_messages(["Offline archive smoke", "Live WAL follow-up"])
            config = json.loads(run("docker", "inspect", container))[0]
            assert config["State"]["Running"], config["State"]
            assert config["Config"]["User"] == "65532:65532"
            assert config["HostConfig"]["ReadonlyRootfs"]
            assert config["HostConfig"]["CapDrop"] == ["ALL"]
            assert any(option.startswith("no-new-privileges") for option in config["HostConfig"]["SecurityOpt"])
            assert all(not mount["RW"] for mount in config["Mounts"] if mount["Type"] == "bind")
            print(f"PASS {architecture} WhatsApp archive: offline native fixture, health/401/authenticated reads, "
                  "live WAL updates, fresh reads/restart after writer exit, read-only mounts/non-root/internal network")
        except BaseException:
            subprocess.run(["docker", "logs", container], check=False)
            raise


def main():
    architecture = run("docker", "info", "--format", "{{.Architecture}}")
    arm = architecture in ("aarch64", "arm64")
    assert arm or architecture in ("x86_64", "amd64"), architecture
    expected = "arm64" if arm else "amd64"
    for image in ("ferret", "ferret-plugins", "ferret-whatsapp-archive", "ferret-frontend"):
        actual = run("docker", "image", "inspect", f"{image}:local", "--format", "{{.Architecture}}")
        assert actual == expected, (image, actual, expected)

    with tempfile.TemporaryDirectory(prefix="ferret-smoke-") as state:
        state = Path(state)
        # Docker Desktop must be able to traverse the bind mount as the image's non-root user.
        state.chmod(0o755)
        smoke_whatsapp_archive(state, expected)
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
            fonts = FontPreloads()
            fonts.feed(html)
            assert len(fonts.hrefs) == 3, fonts.hrefs
            for font in fonts.hrefs:
                assert font.startswith("/_next/static/"), font
                response = request(frontend + font)
                assert response.read(4) == b"wOF2", font
            for family in ("Alegreya", "Geist", "GeistMono"):
                notice = f"fonts/LICENSE-{family}.txt"
                response = request(frontend + "/" + notice)
                assert response.read() == (ROOT / "frontend/public" / notice).read_bytes(), notice
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
