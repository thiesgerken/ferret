# Plugin workers pull runs over HTTP

Connections are configured and scheduled in the server. Plugin workers claim due runs from an
internal HTTP API by long polling and ingest through the public REST API with a per-run token.
Workers have no database access, so the server stays the only component that can read or write
data, and a plugin bug or dependency can't bypass the per-source scoping. Kubernetes CronJobs per
plugin were rejected because per-user configuration and "run now" belong in the app, a message
broker because Postgres (`SKIP LOCKED`, `LISTEN/NOTIFY`) already coordinates, and WebSockets
because long polling gives the same latency with stateless requests.
