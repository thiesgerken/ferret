# ferret owns the Signal chat archive

Signal has no tool like wacli that is collector, durable chat archive and change journal in one:
signal-cli receives but keeps no message history. ferret therefore ships its own
`ferret-signal-archive`, which consumes a linked signal-cli over JSON-RPC, keeps messages and an
unpruned change journal in SQLite, imports history once from Signal Desktop's JSON export, and
serves the same API shape as the WhatsApp archive. Headless Signal Desktop was rejected because it
has no headless mode, its builds expire after 90 days and its database schema changes monthly;
presage because it cannot import history yet, has no journal and is AGPL; signalmeow because it is
a bridge-internal Go library without a stable API, AGPL as well; and ferret's documents as the only
copy because nothing raw would remain to re-segment or reconcile against. The price is a stateful
service whose database is the only raw copy, and a signal-cli sidecar that must be upgraded at
least monthly. See [the design](../conception/signal.md).
