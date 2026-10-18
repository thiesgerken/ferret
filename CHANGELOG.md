# CHANGELOG


## v0.16.0 (2026-10-18)


### ✨ Features


- ✨Merge pull request #141 from thiesgerken/140-model-registry
  ([`5b00b49`](https://github.com/thiesgerken/ferret/commit/5b00b49778cb162ee746adb589abe2a50601dca7))

- ✨ feat(embeddings): model registry, stored embeddings in Admin, delete for unconfigured models
  ([`5b00b49`](https://github.com/thiesgerken/ferret/commit/5b00b49778cb162ee746adb589abe2a50601dca7))

- ✨ feat(embeddings): a model registry with a dimension check, stored embeddings in Admin, and delete for models that aren't configured
  ([`34b9889`](https://github.com/thiesgerken/ferret/commit/34b988986becbebfdd650152053d35e73b7764fc))

  `embedding_models` records every model that ever embedded, seeded from chunk_embeddings and embedding_cache. The server refuses to start when a configured model is known with another dimension, or would share another stored model's index name. Admin lists every registered model with its embedding count, its HNSW index size and an estimate of its rows, and deletes the embeddings, cache entries, failure records, index and registry entry of a model that isn't configured.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### ♻️ Refactoring


- ♻️ refactor(embeddings): foreign keys tie embeddings to the model registry
  ([`87efe91`](https://github.com/thiesgerken/ferret/commit/87efe911ee59b8fbf9cd03e93e058fc93f558029))

  A replica still configured for a deleted model (a rolling deploy) now fails to write instead of leaving rows no registry entry accounts for, and deleting the entry cascades to the model's embeddings, cache entries and failure records. The migration drops failure records of models that never embedded. Also: the Delete button is disabled while a delete runs, and ModelConfig::slug gives way to the free function.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.15.1 (2026-10-18)


### Other


- Merge pull request #138 from thiesgerken/brand/soft-mask-logo
  ([`6eb6ca1`](https://github.com/thiesgerken/ferret/commit/6eb6ca17006871aeba22917d9817618deeb43c30))

- Merge pull request #135 from thiesgerken/docs/operations-development
  ([`ce691dc`](https://github.com/thiesgerken/ferret/commit/ce691dc01afc32ce5536325ec7e40acaf1afba52))

  📝 docs(operations): complete deployment and development guides

- 📝 docs(operations): correct credential failure recovery lifecycle
  ([`db517c7`](https://github.com/thiesgerken/ferret/commit/db517c72ed878d7a8d7ad217f7ac0a783c832cb7))

- 📝 docs(operations): complete deployment and development guides
  ([`d283e55`](https://github.com/thiesgerken/ferret/commit/d283e553c0b1e6b1e20242d567f781ff4112a5d9))

- Merge pull request #134 from thiesgerken/docs/technical-reference
  ([`7135527`](https://github.com/thiesgerken/ferret/commit/713552743bdba1f7070e9288cc278e4b00bcd824))

  📝 docs(reference): document architecture APIs and plugin contract

- 📝 docs(api): describe shipped MCP search pagination
  ([`e38b3ee`](https://github.com/thiesgerken/ferret/commit/e38b3eed6ab9a4a261471f73e5a9580a34a917d9))

- 📝 docs(reference): document architecture APIs and plugin contract
  ([`3a90c99`](https://github.com/thiesgerken/ferret/commit/3a90c9949f5a05ee8d68d592919be8d4be5b89e5))

- Merge pull request #133 from thiesgerken/docs/getting-started
  ([`68dbb75`](https://github.com/thiesgerken/ferret/commit/68dbb754aa8a238b7ad803ce5f0af917b7b1dbd2))

  📝 docs(start): document quickstart and integrations

- 📝 docs(start): match English search mode labels
  ([`4b13eae`](https://github.com/thiesgerken/ferret/commit/4b13eae33e54636017375bffdfc1558d9168965f))

- 📝 docs(mcp): document shipped transport and verified tools
  ([`afd0946`](https://github.com/thiesgerken/ferret/commit/afd0946ee7fd2fa5159ba6ca86db66cbc16b4299))

- Merge remote-tracking branch 'origin/main' into docs/getting-started
  ([`9cc1189`](https://github.com/thiesgerken/ferret/commit/9cc11890029fd2e676ca4a8f6e0301e242624cbf))

### 💄 UI/UX


- 💄 style(brand): adopt the soft-mask ferret logo
  ([`6eb6ca1`](https://github.com/thiesgerken/ferret/commit/6eb6ca17006871aeba22917d9817618deeb43c30))

- 💄 style(brand): adopt the soft-mask ferret logo
  ([`0403e57`](https://github.com/thiesgerken/ferret/commit/0403e5737c78c44b9b3265fc2e84b4f62cb4f862))

## v0.15.0 (2026-10-17)


### Other


- Merge pull request #131 from thiesgerken/128-mcp
  ([`da4c1c6`](https://github.com/thiesgerken/ferret/commit/da4c1c61163b26ac3f22438ec5d9566eb7aeee61))

- Merge remote-tracking branch 'origin/main' into 128-mcp
  ([`b8d30d4`](https://github.com/thiesgerken/ferret/commit/b8d30d487d00944ad9196a8d2a12eb6037c158f4))

  # Conflicts: #	Cargo.lock #	crates/ferret-server/Cargo.toml

- 📝 docs(start): use real thread navigation controls
  ([`f9816b0`](https://github.com/thiesgerken/ferret/commit/f9816b0d8e2d8d84d906293d8e5ce03a897c724b))

- 📝 docs(connections): clarify exclusion restoration and ownership
  ([`394faac`](https://github.com/thiesgerken/ferret/commit/394faac9049c854616218ee2b1f24c14aff68841))

- 📝 docs(start): document quickstart and integrations
  ([`f3ef756`](https://github.com/thiesgerken/ferret/commit/f3ef756d7f82eb022393d2e9a6b999b29ee69a54))

### ✨ Features


- ✨ feat(server): an MCP server at /mcp with search, get_document and list_sources
  ([`da4c1c6`](https://github.com/thiesgerken/ferret/commit/da4c1c61163b26ac3f22438ec5d9566eb7aeee61))

- ✨ feat(server): an MCP server at /mcp with search, get_document and list_sources
  ([`852a284`](https://github.com/thiesgerken/ferret/commit/852a28445e1b414f896cbb2b9549058e19ceef41))

  Agents with an MCP client use ferret without a skill: Streamable HTTP (rmcp), stateless with JSON responses, authenticated with the API keys of /api (search:read, their sources). The tools call the same search and document code as REST and GraphQL. The HTTPRoute, the compose proxy and next dev route /mcp to the server.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### ♻️ Refactoring


- ♻️ refactor(server): MCP's limit is checked and named as the agent sees it, and the docs count MCP
  ([`0ab3445`](https://github.com/thiesgerken/ferret/commit/0ab344522c2412ee7979f60cb53f10b5d4dfd407))

  The search tool rejects an out-of-range limit under its own name and states the range in its schema. The shared service functions' doc comments, auth.md's typical keys and the v1 scope now include MCP, and rmcp no longer sits under reqwest's comment.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.14.0 (2026-10-17)


### Other


- Merge pull request #116 from thiesgerken/112-imap
  ([`9842058`](https://github.com/thiesgerken/ferret/commit/9842058ffcfdd48b0708945bb364841407ea5fe4))

- 🔀 Merge main into 112-imap, with AFFiNE next to IMAP
  ([`28d4b1b`](https://github.com/thiesgerken/ferret/commit/28d4b1bcc34080fe221f00aac032bc84a15c7a39))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### ✨ Features


- ✨ feat(plugins): index a mailbox over IMAP
  ([`9842058`](https://github.com/thiesgerken/ferret/commit/9842058ffcfdd48b0708945bb364841407ea5fe4))

## v0.13.0 (2026-10-17)


### Other


- Merge pull request #118 from thiesgerken/113-affine-plugin
  ([`788d640`](https://github.com/thiesgerken/ferret/commit/788d64054db56da4b46e6d60ccf4a15183d22fc4))

- 📝 docs(plugins): what goes stale in AFFiNE documents, and why there's no meta
  ([`f770b6b`](https://github.com/thiesgerken/ferret/commit/f770b6bbdac5311227e142d84adf2ae86565a129))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✅ test(plugins): sync the AFFiNE plugin against a fake AFFiNE
  ([`e97ca41`](https://github.com/thiesgerken/ferret/commit/e97ca4110e0c2156daca5c33574d05fa477193ea))

  Pages of three with a tie across a page's end, the cursor and its overlap, trash and restore, a doc without content, a page of one modification time, and a login proxy in front of AFFiNE.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🔀 Merge main into 112-imap, with the test Dovecot in docs/development.md
  ([`d29765e`](https://github.com/thiesgerken/ferret/commit/d29765e9f959f842e1eab62cbf076218272dd449))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### ✨ Features


- ✨ feat(plugins): index AFFiNE workspaces
  ([`788d640`](https://github.com/thiesgerken/ferret/commit/788d64054db56da4b46e6d60ccf4a15183d22fc4))

- ✨ feat(plugins): index AFFiNE workspaces
  ([`6c8f2a4`](https://github.com/thiesgerken/ferret/commit/6c8f2a49f52c5545686a363cae1ec2b963801f50))

  An `affine` plugin: one Markdown document per doc of a self-hosted AFFiNE workspace. It signs in as a user (AFFiNE 0.27 has no personal access tokens for its API), lists the docs and their trash flags from the workspace's root doc, finds changed docs through GraphQL `recentlyUpdatedDocs`, and turns each doc's Yjs state into Markdown with its own converter on `yrs`. Every run is a sync, so deleted and trashed docs leave the index.

  Closes #113

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 🐛 Bug Fixes


- 🐛 fix(plugins): fetch only restored AFFiNE docs and guard against block cycles
  ([`5130707`](https://github.com/thiesgerken/ferret/commit/51307074a6339ccced45e94f482efc912004e563))

  - A doc that left the trash is fetched by itself, with its updatedAt from
    `workspace.doc(docId)`, instead of a full fetch of the workspace.
  - Paging follows AFFiNE's own hasNextPage, so it doesn't depend on the page
    size the server actually serves. Only a full page of one modification time
    fails the run.
  - A full fetch logs how many docs have no content on the server yet.
  - The converter renders a block among its own descendants once instead of
    recursing until the worker's stack overflows.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.12.5 (2026-10-17)


### Other


- Merge pull request #127 from thiesgerken/readme
  ([`aa4b653`](https://github.com/thiesgerken/ferret/commit/aa4b653845f75df3e05103e4c351fa392c7f57eb))

  📝 docs: a README that says what ferret is for

- 📝 docs: an agents-first README intro, with IMAP, AFFiNE and MCP
  ([`3f28aae`](https://github.com/thiesgerken/ferret/commit/3f28aae6dc9cdbce987461be8fdd11519eac3044))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs: ferret as the discovery layer in the README's intro
  ([`d5aeaab`](https://github.com/thiesgerken/ferret/commit/d5aeaab80d2f2abcfd82b1778e95162a7d8a9495))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs: the README's build badge follows the release workflow on main
  ([`03d42ba`](https://github.com/thiesgerken/ferret/commit/03d42ba9d4d88136fb3d19374a6bc4a63206af70))

  ci.yml only runs on pull requests, so its badge showed whichever PR ran last.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs: the README's screenshots in dark mode too
  ([`4a8a11a`](https://github.com/thiesgerken/ferret/commit/4a8a11a6b3c753247ec3ccb408dd4d359f3b8a76))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs: ferret as the agent's discovery tool
  ([`5c79f33`](https://github.com/thiesgerken/ferret/commit/5c79f330d084510bd1dc7d53976c242c598ca039))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs: Thies' motivation, and the highlights tied to it
  ([`ee512cb`](https://github.com/thiesgerken/ferret/commit/ee512cbdfd109095911b983f369e66837b4c7fac))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs: the README's logo without a heading rule
  ([`da8c760`](https://github.com/thiesgerken/ferret/commit/da8c760eb78c55e59ecf6b69ab422a5c51755832))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs: a README that says what ferret is for
  ([`e020af1`](https://github.com/thiesgerken/ferret/commit/e020af1768be7e1994b1b50116154f35068f3dc9))

  The lockup on top, highlights, motivation, screenshots, sources, getting started and a docs table, in carapace's shape. Development details move to docs/development.md.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 🔧 Configuration


- 🔧 chore(demo): the Harz chat is Thies and Anna only
  ([`6216425`](https://github.com/thiesgerken/ferret/commit/62164255619cecb68f8d67dcd5263d4fc2282570))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 💄 UI/UX


- 💄 style(brand): an outlined wordmark and mark-and-wordmark lockups
  ([`283a984`](https://github.com/thiesgerken/ferret/commit/283a984ec96749453e2fe7c1bf8e1883c2cd052c))

  GitHub can't load Alegreya, so the wordmark ships as outlines: wordmark.py shapes "ferret" in Alegreya SemiBold with HarfBuzz and writes it as SVG paths, alone and next to the mark in light and dark.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.12.4 (2026-10-17)


### Other


- Merge pull request #130 from thiesgerken/pump-wake-followup
  ([`6075288`](https://github.com/thiesgerken/ferret/commit/6075288d7837ff1202597b807be9db16c8aaa3ec))

### 🐛 Bug Fixes


- 🐛 fix(server): the pump looks again after a lost notification and frees a returned claim at once
  ([`6075288`](https://github.com/thiesgerken/ferret/commit/6075288d7837ff1202597b807be9db16c8aaa3ec))

- 🐛 fix(server): the pump looks again after a lost notification and frees a returned claim at once
  ([`77ee66e`](https://github.com/thiesgerken/ferret/commit/77ee66e6375e27ba9ec08c204078f9d216f221aa))

  PgListener::recv reconnects by itself and drops the notifications sent in between without telling anyone: the pumps are now woken whenever the listener (re)connects. A claim returned because the TEI was switched off is rolled back before the other pumps are woken, instead of on drop, which only queued the rollback and could leave them finding the chunks locked.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.12.3 (2026-10-17)


### Other


- Merge pull request #129 from thiesgerken/fix-flaky-tests
  ([`8af33eb`](https://github.com/thiesgerken/ferret/commit/8af33eb4058544e53a855a783274600985a01da7))

- ✅ test(server): order the racing tests by observed state, not by sleeps
  ([`7cdef94`](https://github.com/thiesgerken/ferret/commit/7cdef944f19977894745b4a33f9235c93ea81179))

  The exclusion race test waits for the lock waits in pg_stat_activity instead of sleeping. The model-switch test's fake TEI switches models with the embedding request, so periodic discovery can't see the new model first, and the test waits for the recorded error too.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### 🐛 Bug Fixes


- 🐛 fix(server): the pump hears every new chunk and honours a TEI switched off mid-claim
  ([`8af33eb`](https://github.com/thiesgerken/ferret/commit/8af33eb4058544e53a855a783274600985a01da7))

- 🐛 fix(server): the pump hears every new chunk and honours a TEI switched off mid-claim
  ([`b748361`](https://github.com/thiesgerken/ferret/commit/b748361b1eaa599ba42f55d548c58aa7a1d9e270))

  The pump started claiming before it listened for chunks_pending, so a chunk ingested in between waited for the idle poll. A pump that checked its TEI's switch before an admin turned embedding off could still claim chunks ingested after that; it now checks again after the claim and hands them to the other TEIs.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.12.2 (2026-10-17)


### Other


- Merge pull request #126 from thiesgerken/fix-run-duration
  ([`b63a034`](https://github.com/thiesgerken/ferret/commit/b63a0346a446fe17c7c60cb3c415392cdd306aac))

### 🐛 Bug Fixes


- 🐛 fix(frontend): an API key's running burst reads "for", not "took"
  ([`b63a034`](https://github.com/thiesgerken/ferret/commit/b63a0346a446fe17c7c60cb3c415392cdd306aac))

- 🐛 fix(frontend): an API key's running burst reads "for", not "took"
  ([`29973d9`](https://github.com/thiesgerken/ferret/commit/29973d91f1fb10ee26d8fe2c29a27f1e8e6004df))

  RunDuration chose its wording by whether the run had an end, but an API key's burst has its last write as finishedAt while it's still running, so Activity showed "Running · took 0 sec". The status decides again, as in the feed before #124.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.12.1 (2026-10-17)


### Other


- Merge pull request #125 from thiesgerken/explore-table
  ([`1f17d0b`](https://github.com/thiesgerken/ferret/commit/1f17d0b3f145e4ad95caed1669579681c98c3fdc))

### 💄 UI/UX


- 💄 style(frontend): Explore's table fits the page instead of scrolling sideways
  ([`1f17d0b`](https://github.com/thiesgerken/ferret/commit/1f17d0b3f145e4ad95caed1669579681c98c3fdc))

- 💄 style(frontend): Explore's table fits the page instead of scrolling sideways
  ([`1ea2904`](https://github.com/thiesgerken/ferret/commit/1ea2904a33ad6c41e96378b78544573a1d594208))

  A fixed table layout with truncated cells; narrower screens drop type, authors and the modified date. The page is a bit wider, like a table page can be.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.12.0 (2026-10-17)


### Other


- Merge pull request #124 from thiesgerken/58-i18n-manage
  ([`07f3875`](https://github.com/thiesgerken/ferret/commit/07f3875f733d22b77baf7696f1232ac79f4aefbd))

### ✨ Features


- ✨ feat(frontend): German UI for Activity, Connections, Admin and Explore
  ([`07f3875`](https://github.com/thiesgerken/ferret/commit/07f3875f733d22b77baf7696f1232ac79f4aefbd))

- ✨ feat(frontend): German UI for Activity, Connections, Admin and Explore
  ([`35b2d2e`](https://github.com/thiesgerken/ferret/commit/35b2d2e3cce0350c571edaad26b2c24f04ddf186))

  The rest of the UI moves to the message catalogues. Durations format with Intl.DurationFormat and relative times with the active locale; Explore's months read as "January 2026" / "Januar 2026".

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.11.0 (2026-10-17)


### Other


- Merge pull request #123 from thiesgerken/58-i18n
  ([`99351fb`](https://github.com/thiesgerken/ferret/commit/99351fbbb65bdeda7d7d964e71d62ad72a1e658d))

### ✨ Features


- ✨ feat(frontend): German UI for search, viewer, sign-in and settings
  ([`99351fb`](https://github.com/thiesgerken/ferret/commit/99351fbbb65bdeda7d7d964e71d62ad72a1e658d))

- ✨ feat(frontend): German UI for search, viewer, sign-in and settings
  ([`978e07f`](https://github.com/thiesgerken/ferret/commit/978e07fae8600b8e5fc1de458be4b78c51c38238))

  The UI follows the browser's languages until the user picks one in Settings, stored per browser. next-intl with catalogues in frontend/messages, as in carapace; a test fails when a key is missing in either. Dates and numbers format with the active locale.

  The home page drops its tagline and counts chunks too, largest first.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.10.0 (2026-10-17)


### Other


- Merge pull request #120 from thiesgerken/117-tei-pool
  ([`89f6dba`](https://github.com/thiesgerken/ferret/commit/89f6dba502157c9f6116ee7ec659d86ccc76791f))

- Merge pull request #122 from thiesgerken/117-tei-pool-ui
  ([`59e2b74`](https://github.com/thiesgerken/ferret/commit/59e2b740e9c184f18893a0085c8f4ee2dd0c24cd))

- Merge pull request #121 from thiesgerken/117-tei-pool-ui
  ([`1535067`](https://github.com/thiesgerken/ferret/commit/15350670e73057628a86cd7c6de997d4b4b71d49))

- 📝 docs: the TEI pool
  ([`349895b`](https://github.com/thiesgerken/ferret/commit/349895b279d7482aa71537ff5cef108ba3198da2))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### ✨ Features


- ✨ feat(server): a TEI pool instead of TEIs per model
  ([`89f6dba`](https://github.com/thiesgerken/ferret/commit/89f6dba502157c9f6116ee7ec659d86ccc76791f))

- ✨ feat(frontend): the TEI pool in Admin and on Activity
  ([`1535067`](https://github.com/thiesgerken/ferret/commit/15350670e73057628a86cd7c6de997d4b4b71d49))

- ✨ feat(frontend): the TEI pool in Admin and on Activity
  ([`1c6343b`](https://github.com/thiesgerken/ferret/commit/1c6343bb1cd0e167a69ad3a81ae8eea48c8b2b4d))

  Admin groups the pool's TEIs under the model each serves, with "Unused (model)" and "Offline" groups for the rest. Each TEI has switches for on, queries and embedding, its state and problem, and its figures. A model says when no enabled TEI answers its queries or embeds its chunks. Activity shows the same TEI cards per model for admins, and every user sees whether a model's queries and embedding are served.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(chart): internal TEIs and externalTeis form the TEI pool
  ([`59583f8`](https://github.com/thiesgerken/ferret/commit/59583f80684dbeb520920fe0dffa974e3aa802c4))

  tei.internal.enabled deploys one CPU TEI per model, externalTeis lists TEIs outside the chart. tei.gpu and the models' bulkUrl are gone, and the chart refuses to render while they're set.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(server): a pool of TEIs that serve whichever model they have
  ([`474cb19`](https://github.com/thiesgerken/ferret/commit/474cb19f4d8c4e808d62360fd1224f022ecca4a3))

  The config lists TEIs without a model. The server asks each one's /info every 10 seconds which model it serves and assigns it to the configured model with that model_id. Embeddings go through /v1/embeddings, and an answer from another model or dimension is discarded and the TEI asked again. Admins switch each TEI on or off, for queries and for the pump (stored in the new teis table). The pump runs one loop per TEI, with short batches on TEIs that also answer queries, and queries go to an idle query TEI first. Replaces url and bulk_url per model.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### ♻️ Refactoring


- ♻️ refactor(frontend): a model's dimension is always there
  ([`59e2b74`](https://github.com/thiesgerken/ferret/commit/59e2b740e9c184f18893a0085c8f4ee2dd0c24cd))

- ♻️ refactor(frontend): a model's dimension is always there
  ([`ce1ae5b`](https://github.com/thiesgerken/ferret/commit/ce1ae5beb96e7f0e030f7f3610599d98b34eef52))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 🐛 Bug Fixes


- 🐛 fix(server): configured model dimensions, checked TEI names and race-free switches
  ([`277af1e`](https://github.com/thiesgerken/ferret/commit/277af1e41df35d052e79fb8ba3c152d026c13c8d))

  Every model's dimension is configured: the index is built for it at startup, a mismatch with stored embeddings stops the server, and answers of another dimension are discarded. A TEI serving its model under another name than its model id is unsuitable, since its answers couldn't show a model switch. Discovery and switches update a TEI's state under one lock, and switches from the database apply only when they changed there, so a poll can't undo an admin's change. Rediscovery after a mismatch runs in the background.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### 🔧 Configuration


- 🔧 chore(compose): tei and tei-bulk as a TEI pool
  ([`d0e956b`](https://github.com/thiesgerken/ferret/commit/d0e956bcf6ca12702f0ffc3b02cd34c5b69980b4))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.9.1 (2026-10-17)


### Other


- Merge pull request #119 from thiesgerken/wordmark-alegreya
  ([`f0b74d8`](https://github.com/thiesgerken/ferret/commit/f0b74d8cefdca1584e19793e02a4672f2f9e407c))

### 💄 UI/UX


- 💄 style(frontend): set the wordmark in Alegreya SemiBold
  ([`f0b74d8`](https://github.com/thiesgerken/ferret/commit/f0b74d8cefdca1584e19793e02a4672f2f9e407c))

- 💄 style(frontend): set the wordmark in Alegreya SemiBold
  ([`6d923a4`](https://github.com/thiesgerken/ferret/commit/6d923a4b1e331a447bc319bdb00562fafd636149))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01UpMJXTPNdGsxLCBbn59gv9

- 💄 style(frontend): set the wordmark in Alegreya Bold
  ([`9cf4e83`](https://github.com/thiesgerken/ferret/commit/9cf4e831dcc3025310ce03ae2c48d5dc0f87ce39))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01UpMJXTPNdGsxLCBbn59gv9

## v0.9.0 (2026-10-17)


### Other


- Merge pull request #115 from thiesgerken/chart-bulk-url
  ([`ebe4d48`](https://github.com/thiesgerken/ferret/commit/ebe4d48606e8b6ccd9807690c7fb5d5818467312))

### ✨ Features


- ✨ feat(chart): an external TEI per model for backfills
  ([`ebe4d48`](https://github.com/thiesgerken/ferret/commit/ebe4d48606e8b6ccd9807690c7fb5d5818467312))

- ✨ feat(chart): an external TEI per model for backfills
  ([`8e08a0a`](https://github.com/thiesgerken/ferret/commit/8e08a0a68b25b5de2f91215c90f74b8cbaa35db1))

  A model's bulkUrl renders into the config's bulk_url, so the embed pump can use a TEI outside the cluster (e.g. a laptop's GPU in the LAN) for a backfill. The render fails when it is set together with tei.gpu.enabled.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.8.1 (2026-10-17)


### Other


- Merge pull request #114 from thiesgerken/source-help-text
  ([`a3614e3`](https://github.com/thiesgerken/ferret/commit/a3614e35ecb32ca16af5e165a62ed25185d6f5fb))

- 📝 docs(plugins): IMAP folder matching, ordering and refetches
  ([`56bdd1d`](https://github.com/thiesgerken/ferret/commit/56bdd1da7a0a93d629120460a70aab900f75550d))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### 💄 UI/UX


- 💄 style(frontend): explain what a connection's source is for
  ([`a3614e3`](https://github.com/thiesgerken/ferret/commit/a3614e35ecb32ca16af5e165a62ed25185d6f5fb))

- 💄 style(frontend): explain what a connection's source is for
  ([`b1bd587`](https://github.com/thiesgerken/ferret/commit/b1bd58700b7dc059fdcee58b145e2bfa9ddfd747))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 🔧 Configuration


- 🔧 chore(compose): scale the test Dovecot to 0 by default
  ([`c06fd65`](https://github.com/thiesgerken/ferret/commit/c06fd65db7928c34007c311f385076c82f1b0b27))

  The Dovecot is only for testing the IMAP plugin, so a plain `docker compose up` leaves it out. CI and the docs start it with `--scale dovecot=1`.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 🐛 Bug Fixes


- 🐛 fix(plugins): match excluded folders by name segment and order mails by Date
  ([`5105909`](https://github.com/thiesgerken/ferret/commit/510590916ccf5790ad777dcf6d14fde4f61060f7))

  Excluded folders compare with the last segment of a folder's path, decoded from modified UTF-7, so INBOX.Trash and Entwürfe match. New mails are put in the order of their Date (else arrival), the date the thread diff compares, and each thread is asked from ferret once per run.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### 🔒 Security


- 🔒 fix(plugins): keep async-imap's command traces out of the logs
  ([`865116d`](https://github.com/thiesgerken/ferret/commit/865116dabcad19e13913cb139eb2eb585069296e))

  async-imap traces every command it sends, including LOGIN with the password, and RUST_LOG=trace would put it into the worker's logs.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### ✨ Features


- ✨ feat(plugins): index a mailbox over IMAP
  ([`2e32564`](https://github.com/thiesgerken/ferret/commit/2e325647b8ecd06c1400846e4b6465c2d8d7f543))

  One document per mail, identified by its Message-ID, in the connection's root (e.g. mail/zoho). Every run is a sync over the account's folders, fetching only mails no earlier run indexed; moves and copies aren't fetched again, a new UIDVALIDITY refetches its folder. Quoted text is stripped by HTML containers, text markers and a diff against the thread's earlier mails. Tests run against a throwaway Dovecot.

  Fixes #112

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.8.0 (2026-10-17)


### Other


- Merge pull request #111 from thiesgerken/search-mode-ui
  ([`385f8d0`](https://github.com/thiesgerken/ferret/commit/385f8d02a49a2ce4c074847e4b448eae250ab233))

### ✨ Features


- ✨ feat(frontend): switch keyword and semantic matching on and off
  ([`385f8d0`](https://github.com/thiesgerken/ferret/commit/385f8d02a49a2ce4c074847e4b448eae250ab233))

- ✨ feat(frontend): switch keyword and semantic matching on and off
  ([`99564ce`](https://github.com/thiesgerken/ferret/commit/99564ce74485a1c148a9f897b87397f3a56c03e1))

  The results page's filters get a "Matching" group with Keyword and Semantic toggles, both on by default. The one left on can't go off. The choice lives in the URL as mode=keyword|semantic (hybrid stays out of it) and goes to the search as its mode. The stats line says "keyword matches" or "semantic matches", the empty state names the mode's rule, and the model select hides while only keyword matching runs.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.7.0 (2026-10-17)


### Other


- Merge pull request #110 from thiesgerken/search-mode
  ([`82df373`](https://github.com/thiesgerken/ferret/commit/82df373aba761188531520944669af48bd5b0147))

### ✨ Features


- ✨ feat(server): run keyword or semantic search alone
  ([`82df373`](https://github.com/thiesgerken/ferret/commit/82df373aba761188531520944669af48bd5b0147))

- ✨ feat(server): run keyword or semantic search alone
  ([`f540c3e`](https://github.com/thiesgerken/ferret/commit/f540c3ecfe71494023695bd714dfcadb849242dd))

  A `mode` on search (REST `mode=`, GraphQL `SearchInput.mode`): HYBRID by default, KEYWORD skips the query embedding, SEMANTIC runs only the vector half and fails with 503 / UNAVAILABLE without the embedding model. Ranking, total, facets, paging and the lower-bound rule come from the halves that ran.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.6.5 (2026-10-17)


### Other


- Merge pull request #109 from thiesgerken/viewer-header-align
  ([`9a83a1f`](https://github.com/thiesgerken/ferret/commit/9a83a1f6b55a34995751a8169ae948dae5572f68))

### 💄 UI/UX


- 💄 style(frontend): line up the viewer's source and modified date
  ([`9a83a1f`](https://github.com/thiesgerken/ferret/commit/9a83a1f6b55a34995751a8169ae948dae5572f68))

- 💄 style(frontend): line up the viewer's source and modified date
  ([`20fdb8c`](https://github.com/thiesgerken/ferret/commit/20fdb8c89447225a6918cf27152ffc9f974cd169))

  Inline, the source's icon set the inline-flex box's baseline and lifted the writer above the "· modified" text. The line is a flex row now.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.6.4 (2026-10-17)


### Other


- Merge pull request #108 from thiesgerken/home-search-placeholder
  ([`91f2ada`](https://github.com/thiesgerken/ferret/commit/91f2adafaa686b6e648b3b8c43f13d4bbfe11230))

### 💄 UI/UX


- 💄 style(frontend): drop the example query from the home search box
  ([`91f2ada`](https://github.com/thiesgerken/ferret/commit/91f2adafaa686b6e648b3b8c43f13d4bbfe11230))

- 💄 style(frontend): drop the example query from the home search box
  ([`a15ee91`](https://github.com/thiesgerken/ferret/commit/a15ee91cd2073bde2e455735f5b1ba35c09f4510))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.6.3 (2026-10-17)


### Other


- Merge pull request #107 from thiesgerken/fix/httproute-defaults
  ([`cdd213e`](https://github.com/thiesgerken/ferret/commit/cdd213e959076d6950a5ea1888f66efc05875931))

### 🐛 Bug Fixes


- 🐛 fix(chart): spell out HTTPRoute backendRef defaults so GitOps stays in sync
  ([`cdd213e`](https://github.com/thiesgerken/ferret/commit/cdd213e959076d6950a5ea1888f66efc05875931))

- 🐛 fix(chart): spell out HTTPRoute backendRef defaults so GitOps stays in sync
  ([`cc21370`](https://github.com/thiesgerken/ferret/commit/cc213701873e2cebfd889fa042b806d867c46321))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01Rb9qF1yxAv3fNUpxRXCieL

## v0.6.2 (2026-10-17)


### Other


- Merge pull request #106 from thiesgerken/connections-polish
  ([`9c11692`](https://github.com/thiesgerken/ferret/commit/9c116926d82501c2e05f46d8d52b754a9659008a))

### 💄 UI/UX


- 💄 style(frontend): bring Connections up to the Activity page's level
  ([`9c11692`](https://github.com/thiesgerken/ferret/commit/9c116926d82501c2e05f46d8d52b754a9659008a))

- 💄 style(frontend): bring Connections up to the Activity page's level
  ([`007a2e3`](https://github.com/thiesgerken/ferret/commit/007a2e318f9344579fdf5d0b076c2c6d5b1e16b3))

  Connection cards lead with the writer's icon and name, show the last run, its changes, the next run and the run history with the same pieces as Activity (shared in run-status.tsx), and link to the connection's feed on Activity instead of repeating its logs. Activity's writer filter moves into the URL (?writer=<id>) for that link.

  The create and edit forms are split into sections with help text, schedule presets, and the plugin's config fields with their descriptions' code spans.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.6.1 (2026-10-17)


### Other


- Merge pull request #105 from thiesgerken/writers-in-documents-ui
  ([`e94e4b8`](https://github.com/thiesgerken/ferret/commit/e94e4b818493a2de291f19a4a92561adcd4b08b8))

- Merge remote-tracking branch 'origin/main' into writers-in-documents-ui
  ([`73bde54`](https://github.com/thiesgerken/ferret/commit/73bde54ccd77e33982d22e57e0604c2b92cf0305))

- Merge remote-tracking branch 'origin/writer-on-documents' into writers-in-documents-ui
  ([`dfc71bd`](https://github.com/thiesgerken/ferret/commit/dfc71bdfe47a2f318dca2f3e37fe635b72440d07))

### 💄 UI/UX


- 💄 style(ui): Explore's table and the viewer show a document's writer
  ([`e94e4b8`](https://github.com/thiesgerken/ferret/commit/e94e4b818493a2de291f19a4a92561adcd4b08b8))

- 💄 style(ui): Explore's table and the viewer show a document's writer
  ([`c2082d9`](https://github.com/thiesgerken/ferret/commit/c2082d9a5743556e108c46a85bdd840a3e403e32))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.6.0 (2026-10-17)


### Other


- Merge pull request #103 from thiesgerken/87-activity-ui
  ([`f795170`](https://github.com/thiesgerken/ferret/commit/f795170700a99ef5e0f80b54263de8055c455c72))

- Merge remote-tracking branch 'origin/main' into 87-activity-ui
  ([`ee0bb22`](https://github.com/thiesgerken/ferret/commit/ee0bb22e7a8d5eae6f032a866ddcff684511257b))

### ✨ Features


- ✨ feat(ui): an Activity page with the index, the embedders and every ingest, live
  ([`f795170`](https://github.com/thiesgerken/ferret/commit/f795170700a99ef5e0f80b54263de8055c455c72))

## v0.5.1 (2026-10-17)


### Other


- Merge pull request #104 from thiesgerken/ci-release-latest-main
  ([`2d6eb23`](https://github.com/thiesgerken/ferret/commit/2d6eb23863bb483d5ad5cc99ada8eff562a4743b))

### 👷 CI/Build


- 👷 ci(release): version the tip of main, not the pushed commit
  ([`2d6eb23`](https://github.com/thiesgerken/ferret/commit/2d6eb23863bb483d5ad5cc99ada8eff562a4743b))

- 👷 ci(release): version the tip of main, not the pushed commit
  ([`6e5c42c`](https://github.com/thiesgerken/ferret/commit/6e5c42cf07a9ffcbb36b327885d991709d71b161))

  Back-to-back merges made the earlier Release run fail with "Upstream branch 'origin/main' has changed". Checking out main's tip lets that run release every merge and the later run find nothing to release.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.5.0 (2026-10-17)


### Other


- Merge pull request #102 from thiesgerken/writer-on-documents
  ([`228cc0d`](https://github.com/thiesgerken/ferret/commit/228cc0d612b723c44b99aa952b7dd2fd51d50341))

- Merge pull request #99 from thiesgerken/87-activity
  ([`34f5880`](https://github.com/thiesgerken/ferret/commit/34f5880db9e41849b7c49a4ab6b3ba44ece1af6f))

### ✨ Features


- ✨ feat(server): documents and Explore's rows carry their writer
  ([`228cc0d`](https://github.com/thiesgerken/ferret/commit/228cc0d612b723c44b99aa952b7dd2fd51d50341))

- ✨ feat(server): documents and Explore's rows carry their writer
  ([`6a5eb68`](https://github.com/thiesgerken/ferret/commit/6a5eb68ca62152220dbc2e1f81b4a5da0009cacb))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(server): an activity feed and embedding progress with TEI load
  ([`34f5880`](https://github.com/thiesgerken/ferret/commit/34f5880db9e41849b7c49a4ab6b3ba44ece1af6f))

## v0.4.1 (2026-10-17)


### Other


- Merge pull request #101 from thiesgerken/87-settings-shell
  ([`0e3a63e`](https://github.com/thiesgerken/ferret/commit/0e3a63ea7d093e8490081dfa0f7fb8417464b1fe))

- Merge pull request #100 from thiesgerken/85-followups
  ([`56470eb`](https://github.com/thiesgerken/ferret/commit/56470eb0c618b8f05ee68f8dba1cba1d5306686b))

- Merge branch '87-settings-shell' into 87-activity-ui
  ([`77ff2ec`](https://github.com/thiesgerken/ferret/commit/77ff2ec2769c35c3119288409194d8dae91df28d))

- Merge remote-tracking branch 'origin/main' into 87-settings-shell
  ([`c4688cf`](https://github.com/thiesgerken/ferret/commit/c4688cfafd0f3e05713ced52a71c7a7b051a0d40))

- Merge remote-tracking branch 'origin/main' into 87-activity
  ([`ab8adbb`](https://github.com/thiesgerken/ferret/commit/ab8adbb485aa7a2ca4c0d4b245440a7ad170fbf2))

### 💄 UI/UX


- 💄 style(ui): one layout for Connections, Settings and Admin
  ([`0e3a63e`](https://github.com/thiesgerken/ferret/commit/0e3a63ea7d093e8490081dfa0f7fb8417464b1fe))

- 💄 style(ui): group sources by writer, and load icons only where they show
  ([`56470eb`](https://github.com/thiesgerken/ferret/commit/56470eb0c618b8f05ee68f8dba1cba1d5306686b))

- 💄 style(ui): group sources by writer, and load icons only where they show
  ([`2578cd4`](https://github.com/thiesgerken/ferret/commit/2578cd402091a0b3f10bc942aa5e67fa377394d2))

  The filter column groups a writer's sources under its icon and name. Explore's source facet reads as writers too. Lucide's map of every icon's import and the list of all icon names load lazily, the first when an icon renders and the second when a picker opens, which takes 59 KB gzip off the first load of the search, Connections and Settings pages. A selected parent above a connection's root reads as that connection, and sources no longer flash the full path before the connection roots arrive.

  Part of #85

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 💄 style(ui): one layout for Connections, Settings and Admin
  ([`4708f38`](https://github.com/thiesgerken/ferret/commit/4708f388dbe62a4b0eed564765cb5673c800598f))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### ✨ Features


- ✨ feat(ui): an Activity page with the index, the embedders and every ingest, live
  ([`ae9a3cd`](https://github.com/thiesgerken/ferret/commit/ae9a3cd79b92fdb45a67a59ddb83690d343f7983))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 🐛 Bug Fixes


- 🐛 fix(server): activity keyset paging, one backlog per request, TEI restarts
  ([`228bae2`](https://github.com/thiesgerken/ferret/commit/228bae20d9d14f9e2b3daaae321d45d88ab41f39))

  Part of #87.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.4.0 (2026-10-17)


### Other


- Merge pull request #97 from thiesgerken/85-origins
  ([`dace5b7`](https://github.com/thiesgerken/ferret/commit/dace5b7c4b27ef82b088556065a5c4d75f98007a))

- Merge pull request #98 from thiesgerken/85-origins-ui
  ([`3c6819b`](https://github.com/thiesgerken/ferret/commit/3c6819bd3e45493883d85c466fca833efd8f5609))

- Merge remote-tracking branch 'origin/85-origins' into 85-origins-ui
  ([`b234112`](https://github.com/thiesgerken/ferret/commit/b234112afbbfe0e5421dc4df9196627460805404))

- Merge branch '85-origins' into 87-activity
  ([`82f5433`](https://github.com/thiesgerken/ferret/commit/82f5433a1d8ab245faa2d5d5b5216fa88604e8e5))

- Merge remote-tracking branch 'origin/main' into 85-origins
  ([`8e84d2c`](https://github.com/thiesgerken/ferret/commit/8e84d2cc97df85100436529a0c206ef36416b440))

### ✨ Features


- ✨ feat(server): connections and API keys get a name and an icon, and hits carry their writer
  ([`dace5b7`](https://github.com/thiesgerken/ferret/commit/dace5b7c4b27ef82b088556065a5c4d75f98007a))

- ✨ feat(ui): names and icons for connections and API keys, shown wherever their documents are
  ([`3c6819b`](https://github.com/thiesgerken/ferret/commit/3c6819bd3e45493883d85c466fca833efd8f5609))

- ✨ feat(ui): names and icons for connections and API keys, shown wherever their documents are
  ([`0c4020f`](https://github.com/thiesgerken/ferret/commit/0c4020fc5b72259d57f09ce46c536f2f6759a6b1))

  Connections and API keys get a name and an icon in their forms and lists, with a per-plugin default icon and the icon picker. A hit's source line and the source filters show the writer's icon and name, then the path below a connection's root, with the full source path in a tooltip. The home screen counts writers as sources.

  Part of #85

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(ui): an icon picker over lucide's icons, loaded one by one
  ([`89fc8e6`](https://github.com/thiesgerken/ferret/commit/89fc8e6aabe5d06c6ba409fc36d360eb18fbf38a))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F (cherry picked from commit 76e4e79117ba3d93148928fc624433d2dc5b5379)

- ✨ feat(server): an activity feed and embedding progress with TEI load
  ([`343f473`](https://github.com/thiesgerken/ferret/commit/343f473687ac71a2e7a2a8eb3448844791d46ae8))

  Part of #87.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(server): count what runs and API keys write as it arrives
  ([`1900671`](https://github.com/thiesgerken/ferret/commit/1900671fe3caabaff78526311fdbe8a08bed0a7f))

  Part of #87.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### ♻️ Refactoring


- ♻️ refactor(ui): read the API's writer, as Origin is now called
  ([`0c95e54`](https://github.com/thiesgerken/ferret/commit/0c95e54f14288060eab43841ac8b5be973032226))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ♻️ refactor(server): connections and API keys are writers, with one writer per facet or none
  ([`7354de2`](https://github.com/thiesgerken/ferret/commit/7354de233f7337beb1daf849d4308e31996e217c))

  Origin already means the external system. Part of #85.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.3.1 (2026-10-17)


### Other


- Merge pull request #96 from thiesgerken/ci-helm-pr-version
  ([`1959f8d`](https://github.com/thiesgerken/ferret/commit/1959f8df284db1a13461556deb4d97eea91dc3ef))

### 👷 CI/Build


- 👷 ci: prefix the SHA in the pinned PR chart version
  ([`1959f8d`](https://github.com/thiesgerken/ferret/commit/1959f8df284db1a13461556deb4d97eea91dc3ef))

- 👷 ci: prefix the SHA in the pinned PR chart version
  ([`bf06adc`](https://github.com/thiesgerken/ferret/commit/bf06adcfb402a0038c03667395a6ef23b9e3c631))

  helm refused `0.0.0-pr.88.0751212`: SemVer rejects an all-digit pre-release segment with a leading zero, which hit one PR commit in 16.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

## v0.3.0 (2026-10-17)


### Other


- Merge pull request #92 from thiesgerken/84-facets-ui
  ([`c1e9c4c`](https://github.com/thiesgerken/ferret/commit/c1e9c4c9aa80f4dcbe4a3fe887b4d8a95e7bfdf5))

- Merge remote-tracking branch 'origin/84-search-facets' into 84-facets-ui
  ([`41d6140`](https://github.com/thiesgerken/ferret/commit/41d61402bfc8ebef07daab9708454ef5d42a1d54))

- Merge remote-tracking branch 'origin/84-search-facets' into 84-facets-ui
  ([`3090986`](https://github.com/thiesgerken/ferret/commit/3090986747d7214a6926d27f3603d39e718d7575))

- Merge remote-tracking branch 'origin/integration/v1' into 84-facets-ui
  ([`66349e9`](https://github.com/thiesgerken/ferret/commit/66349e9b010a1fa7e55debde6004a6fc9adfb775))

- Merge pull request #88 from thiesgerken/84-search-facets
  ([`85b708f`](https://github.com/thiesgerken/ferret/commit/85b708f53568d312715035e64c9629db221c4264))

### ✨ Features


- ✨ feat(ui): endless scrolling, match counts in the filters, the model's own cut-off
  ([`c1e9c4c`](https://github.com/thiesgerken/ferret/commit/c1e9c4c9aa80f4dcbe4a3fe887b4d8a95e7bfdf5))

- ✨ feat(ui): endless scrolling, match counts in the filters, the model's own cut-off
  ([`d02ea8c`](https://github.com/thiesgerken/ferret/commit/d02ea8cbfa4732579c606dc29a6710f5c7aaeec9))

  The next page of hits loads when the end of the list scrolls into view, without repeating a hit the index shifted onto a later page, and a quiet marker says when the list ends, or when it ends at the search's candidate limit. The filter column and chips list the matches' sources and authors from the search's facets, with their counts. The relevance bars stretch from the cut-off of the model that ran instead of a fixed 0.42.

  Part of #84

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(search): facet counts, paging and the model's cut-off for the results page
  ([`85b708f`](https://github.com/thiesgerken/ferret/commit/85b708f53568d312715035e64c9629db221c4264))

## v0.2.0 (2026-10-17)


### Other


- Merge pull request #95 from thiesgerken/menu-icons
  ([`53147e0`](https://github.com/thiesgerken/ferret/commit/53147e0e6f4b9c6d36544e8781588962f22aecbf))

- Merge pull request #94 from thiesgerken/integration/v1
  ([`0e595c0`](https://github.com/thiesgerken/ferret/commit/0e595c0740be2122a8d58e1cc575c52fd8343a1b))

- Merge pull request #93 from thiesgerken/84-plugin-titles
  ([`fef065f`](https://github.com/thiesgerken/ferret/commit/fef065f8eb09b6af4dbfd2ab4c7713a25b53c6b6))

### 💄 UI/UX


- 💄 style(ui): icons in the app menu, and a way back to search
  ([`53147e0`](https://github.com/thiesgerken/ferret/commit/53147e0e6f4b9c6d36544e8781588962f22aecbf))

- 💄 style(ui): icons in the app menu, and a way back to search
  ([`1c96fb1`](https://github.com/thiesgerken/ferret/commit/1c96fb1570cdef28d0efe5e79c6efba682e0b4f1))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### ✨ Features


- ✨ feat(plugins): readable file titles and a carapace skills sub-source
  ([`0e595c0`](https://github.com/thiesgerken/ferret/commit/0e595c0740be2122a8d58e1cc575c52fd8343a1b))

- ✨ feat(plugins): readable file titles and a carapace skills sub-source
  ([`fef065f`](https://github.com/thiesgerken/ferret/commit/fef065f8eb09b6af4dbfd2ab4c7713a25b53c6b6))

- ✨ feat(plugins): carapace indexes skills as their own sub-source
  ([`aebf60a`](https://github.com/thiesgerken/ferret/commit/aebf60a0a8b279de26b6e9d8ef2e20bd7dd7a304))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(plugins): Markdown files without a heading are titled by their name in words
  ([`f7902a3`](https://github.com/thiesgerken/ferret/commit/f7902a318b1ee1c525496993b73094902ce3af7a))

  Part of #84.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

## v0.1.0 (2026-10-17)


### ✨ Features


- ✨Merge pull request #22 from thiesgerken/integration/v1
  ([`9b95d4b`](https://github.com/thiesgerken/ferret/commit/9b95d4b07d4f495dc73de31107794af7d80a4295))

  ferret v1

- ✨ feat: connections and API keys get a name and an icon, and hits carry their origin
  ([`2eda11e`](https://github.com/thiesgerken/ferret/commit/2eda11e133e7265a4e0b13f61655cdbeb591ca15))

  Part of #85.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(search): facet counts, paging and the model's cut-off for the results page
  ([`0d8c1e3`](https://github.com/thiesgerken/ferret/commit/0d8c1e30a1aeeb9a2c2c5541e3bcc2f50c7eb152))

  Part of #84.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(ui): search as you type, match stats and per-hit scores
  ([`b9bab9f`](https://github.com/thiesgerken/ferret/commit/b9bab9f76d180ab294c95fcb2c44ecb8a0e0da30))

- ✨ feat(ui): search as you type, match stats and per-hit scores
  ([`a9e560a`](https://github.com/thiesgerken/ferret/commit/a9e560aacf6f4bc5b1e1b67588d94980d31a41fb))

  A stats line above the results says how many documents matched, how many the selected sources hold and how long the search took. Each hit shows its document's best similarity and whether its keywords matched. On the results page the query goes out once typing pauses for 250 ms: the first pause after a submitted query pushes one history entry, later pauses replace it, and a chip or Back within the pause wins over the typed query. The home screen keeps the explicit submit.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(search): similarity cut-off, match counts, per-hit scores and timing
  ([`4424624`](https://github.com/thiesgerken/ferret/commit/4424624a90a69d2b4c5c5ed49a069fba8e01e862))

- ✨ feat(search): similarity cut-off, match counts, per-hit scores and timing
  ([`5d1a29d`](https://github.com/thiesgerken/ferret/commit/5d1a29d8e28defa061704b755a42bbbf3f5d0a71))

  Each embedding model needs a min_similarity. Hits carry keyword_score and similarity instead of the fused score. Part of #72.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(plugins): FERRET_GIT_PROTOCOLS widens the git transports per deployment
  ([`11a7ee9`](https://github.com/thiesgerken/ferret/commit/11a7ee9c36068f2e5724d93127a6a2f7004dd26c))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(ui): the ferret mark as favicon, in the README and next to the name
  ([`a58357e`](https://github.com/thiesgerken/ferret/commit/a58357e1409212237c274bcda169ff7059829e76))

- ✨ feat(ui): the ferret mark as favicon, in the README and next to the name
  ([`cd186f4`](https://github.com/thiesgerken/ferret/commit/cd186f4bced642be3362b09da8f717de32b09356))

  - Favicon set from the brand assets: icon.svg (follows light/dark), favicon.ico (16/32/48) and
    apple-icon.png. The magnifier icon is gone. The theme colour matches the mark's brown.
  - README: the mark at the top, light or dark by GitHub's colour scheme.
  - docs/brand: the mark files and a short guide (colours, clear space, don't outline).
  - The mark sits on the baseline next to "ferret" on the home page, the sign-in page and in the
    headers, drawn in `--brand`, so it follows the theme.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(admin): workers show whether they're online
  ([`76466b9`](https://github.com/thiesgerken/ferret/commit/76466b9e6d69bf758995bfe575367f9f985109e2))

- ✨ feat(admin): workers show whether they're online
  ([`1bd4586`](https://github.com/thiesgerken/ferret/commit/1bd4586aa962f8fa7088c31c6f570dfe77f8a166))

  A worker is online while it asks for runs (an idle one every 30 s) or holds a running run, and offline after two minutes without either. Online workers sort first. A worker that asks again is online again. Offline rows stay listed for a day after they were last seen.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(auth): change your own password in Settings → Account
  ([`2c51427`](https://github.com/thiesgerken/ferret/commit/2c51427fc6c3e67632352fc9ba9936fd662213c6))

- ✨ feat(auth): change your own password in Settings → Account
  ([`fef2682`](https://github.com/thiesgerken/ferret/commit/fef26820bdd74055c84b3f68e1a1b39fbe2e149d))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat: Explore, a document table with facet counts
  ([`f17fba2`](https://github.com/thiesgerken/ferret/commit/f17fba219b1d0113dd8a14336769b94a55a6dc02))

- ✨ feat: Admin section for users, embedding health and workers
  ([`8f5724b`](https://github.com/thiesgerken/ferret/commit/8f5724bf19b44a8f1462c432c2a58319b44c21fe))

- ✨ feat(admin): a disabled user's connections don't run, document user management
  ([`d92553e`](https://github.com/thiesgerken/ferret/commit/d92553ec757da880d5300c8e16135c531ffd82c3))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(frontend): admin page for users, embedding health and workers
  ([`6f73839`](https://github.com/thiesgerken/ferret/commit/6f73839c29e90c215536fed633363762637766fc))

  - Users: a table with admin and enabled toggles, set password, and a form to create users.
  - Embedding: per model, query and bulk TEI health, the last error, and the backlog per source.
  - Plugin workers seen in the last day.
  - Polls every 10 s. The server answers non-admins with "admins only".

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): admin queries for users, workers and embedding status
  ([`bf67831`](https://github.com/thiesgerken/ferret/commit/bf6783107992c3480ee6162dc5e449675bf0b6a8))

  Admins list users, create them, set their passwords (which ends their sessions) and change their display name, admin role and enabled flag. An update that would leave no enabled admin is refused. Admins also see the workers seen in the last day with their plugins, and per embedding model the TEI health (both URLs), the pump's last error and the backlog per source. Everything else gets "admins only".

  Part of #18

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat: exclude documents and threads from the index
  ([`673568d`](https://github.com/thiesgerken/ferret/commit/673568dfa0b02232c8ddce60bcb6173c63a9c5e8))

- ✨ feat(frontend): exclude from index
  ([`e302736`](https://github.com/thiesgerken/ferret/commit/e3027361815c9fe9216e42c077eda8855ba334ad))

  The viewer excludes a document or its whole thread after a confirmation, and says whether its connection will remember it. A connection's card lists its exclusions and includes them again.

  Part of #14

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): exclude documents and threads from the index
  ([`e78fafa`](https://github.com/thiesgerken/ferret/commit/e78fafafd476c47325b2f2be180a25812889574b))

  `excludeDocument` deletes a document, or its whole thread, and records it on the connection that owns its source. From then on that connection's PUTs for it are dropped and answered with `{outcome: excluded}`, so plugins need no knowledge of the list. `removeExclusion` lets it back in and resets the connection's state, so the next run starts over and even an incremental plugin puts the document again. A document no connection owns is only deleted: nothing is left to drop its next PUT.

  PutResponse becomes a tagged enum, since an excluded put has no id.

  Part of #14

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(chart): optional GPU TEI per model for backfills
  ([`da87fc6`](https://github.com/thiesgerken/ferret/commit/da87fc6eae7e7e4f575941a157c45ad967c414c2))

- ✨ feat(chart): optional GPU TEI per model for backfills
  ([`79303ac`](https://github.com/thiesgerken/ferret/commit/79303acbd6204b446f65bcc6c703597e6899372d))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(plugins): Paperless-ngx
  ([`c9d2602`](https://github.com/thiesgerken/ferret/commit/c9d2602b414cc6d4569aa54f75e450278304fd64))

- ✨ feat(plugins): a Paperless-ngx plugin
  ([`5ba0f7e`](https://github.com/thiesgerken/ferret/commit/5ba0f7e8813ed67469af37ee3329552d0bb91e0d))

  Each Paperless document becomes one document with its OCR text, the correspondent as author, and correspondent, document type, tags and ASN in meta. It links to the document's page in Paperless. Every run lists all ids with their modified time, marks them seen, fetches and puts only those modified since the cursor, and commits, so documents deleted in Paperless disappear. The API token is a write-only config field.

  Parsing is tested against fixtures in the shape of Paperless-ngx's API (version 9). The ingest flow runs end to end against a fake Paperless serving them.

  Part of #12

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(search): default embedding model per user
  ([`eacc3ea`](https://github.com/thiesgerken/ferret/commit/eacc3ea3eb989b44a5e45568a8a087a77415293e))

- ✨ feat(search): default embedding model per user
  ([`db64cd0`](https://github.com/thiesgerken/ferret/commit/db64cd00e001b0e98b3908a1d1e19333c7eeeee2))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat: threads in search and the viewer
  ([`0082cc1`](https://github.com/thiesgerken/ferret/commit/0082cc1ce9bac5dce870a22bff6a832a54c30497))

- ✨ feat(frontend): thread navigation and collapsed hits
  ([`7a3a9f5`](https://github.com/thiesgerken/ferret/commit/7a3a9f53f2fdb6b880e7772e38d34e66f23c12d5))

  A hit says how many more documents of its thread matched, and the viewer links to the previous and next document of the thread.

  Part of #8

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): threads, collapsed in search
  ([`a4aaac9`](https://github.com/thiesgerken/ferret/commit/a4aaac93982577cc14aab7f806e15de6d6c220e7))

  - GET /api/documents?source=&thread= lists a thread's documents oldest first, within the
    caller's sources.
  - Search folds a thread's hits into its best one before k, reports how many it folded in
    `collapsed`, and `collapse=false` turns that off, in REST and GraphQL alike.
  - GraphQL documents know their previous and next document in the thread.
  - An index on (user_id, source, thread, created_at, id) serves both walks.

  Part of #8

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(plugins): carapace
  ([`0a41d62`](https://github.com/thiesgerken/ferret/commit/0a41d62d2a66b410e2cbc4bd58e5f971733e4b06))

- ✨ feat(plugins): the carapace plugin, a full sync per run
  ([`73ef93d`](https://github.com/thiesgerken/ferret/commit/73ef93d57f861065e0519366f5ee6c477a558d9c))

  The carapace plugin clones the knowledge repository and puts sessions, facts, digests and the other files into their sources below the connection's root, in one sync per run. Its sweep removes facts that a changed session file dropped. The git plugin's clone and file reading are now shared with it, until the repository module of #10 replaces them.

  render_fixtures.py now runs in a throwaway environment and leaves carapace's own .venv alone.

  Part of #13

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(plugins): parse carapace's memory mirror
  ([`f0e1f78`](https://github.com/thiesgerken/ferret/commit/f0e1f789e187833cc597ca586fe256d867dc615c))

  Sessions and digests are parsed from the Markdown that carapace's render.py writes: front matter, the prose under the title, list sections, and fact lines with source kind, confidence, durability or "dated until", and refs. Anything that doesn't parse fails with its section and line instead of being skipped.

  They map to documents in <root>/sessions, <root>/facts (one per fact, id from the hash of subject and statement) and <root>/digests, with links into carapace's web UI. memory/facts/*.md is ignored and everything else is a file.

  The fixtures are rendered by carapace itself (render_fixtures.py).

  Part of #13

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(search): titles are searched and embedded with every chunk
  ([`f6c3f90`](https://github.com/thiesgerken/ferret/commit/f6c3f900c705bce008e3cbd3974e08385a5bbb0d))

- ✨ feat(search): titles are searched and embedded with every chunk
  ([`593905e`](https://github.com/thiesgerken/ferret/commit/593905eb8a65a0aae110aa8f82d2ede064478c7e))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(search): relevance judgments with a ✓ per hit
  ([`1cea45a`](https://github.com/thiesgerken/ferret/commit/1cea45a646a53e14f348e89e07900299b81fb14f))

- ✨ feat(search): judge relative ranges as presets, validate judged searches
  ([`b077bbd`](https://github.com/thiesgerken/ferret/commit/b077bbdeefe51e8d02e0a83f14c379db0394929d))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat: judge a hit as the right one for its search
  ([`3f104dd`](https://github.com/thiesgerken/ferret/commit/3f104dd708a1553460140f7d27091331eaac1f59))

  A ✓ on a hit stores a relevance judgment with the query and the search's canonical filters (GraphQL setJudgment), every hit says whether it's judged for the current search, and judgments go with their document.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat: plugin credentials and private git repositories
  ([`48bb6a3`](https://github.com/thiesgerken/ferret/commit/48bb6a34283c3f8403f7ac758cbf7595e18c7fda))

- ✨ feat(frontend): secret config fields
  ([`4cdd38f`](https://github.com/thiesgerken/ferret/commit/4cdd38f2189b9893cfa4679cb9fdd0abd9b64164))

  Write-only fields render as password inputs. In the edit form they start empty and aren't required, since an empty one keeps the stored secret.

  Part of #11

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(plugins): clone private git repositories with a token
  ([`53dddbe`](https://github.com/thiesgerken/ferret/commit/53dddbe251280842f097eef88091c78b45881f79))

  The git plugin's write-only token goes to git as an HTTP basic auth header through the environment, so it shows up neither in process listings nor in the clone. The end-to-end test clones from a git server that rejects requests without it, and checks that an edit without the token keeps it and that it's stored nowhere in plaintext.

  Part of #11

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): encrypt plugin credentials at rest
  ([`6f542a5`](https://github.com/thiesgerken/ferret/commit/6f542a5e36a85abb08535b8596bdf2e5dd901614))

  Config fields a plugin's schema marks writeOnly are split off on save and sealed with XChaCha20-Poly1305 under the new required FERRET_CONFIG_KEY (32 bytes, base64), apart from the config the GraphQL API returns. An update that leaves a credential out keeps the stored one. Only a claimed run gets them back. Credentials that can't be opened (a database restored without its key) fail the run with that reason, and an update has to enter them again.

  Part of #11

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(frontend): edit a connection's config and schedule
  ([`0a15712`](https://github.com/thiesgerken/ferret/commit/0a1571210bba2de2cbf1422dad0dd5b4b960a140))

- ✨ feat(plugins): incremental git runs
  ([`b117704`](https://github.com/thiesgerken/ferret/commit/b117704305e190e9d52090699bdbfe35d276af48))

- ✨ feat(plugins): git runs index only the diff since the last run
  ([`02c3692`](https://github.com/thiesgerken/ferret/commit/02c36927ec74e3377422718e97a3b7f397b64193))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(server): semantic and hybrid search
  ([`2333bb9`](https://github.com/thiesgerken/ferret/commit/2333bb91b05dc131ed93ac5670dc44f4cbfc292f))

- ✨ feat: connections, plugin workers and a git plugin
  ([`e52bddb`](https://github.com/thiesgerken/ferret/commit/e52bddbe41d8361f7cf9056ceb09b69ab3a4fdcf))

- ✨ feat: search by keyword alone when the embedding model is down, and say so
  ([`e071418`](https://github.com/thiesgerken/ferret/commit/e0714184a493e67a8a9a42464a2062796c677c83))

  A query that can't be embedded no longer fails the search: the keyword half answers, semantic_unavailable / semanticUnavailable carries the reason, the server logs a warning and the UI shows a notice. GraphQL's search now returns {hits, semanticUnavailable}.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(server): embedding models, embed pump and hybrid search
  ([`8f270ea`](https://github.com/thiesgerken/ferret/commit/8f270ea7dc45b213b1285b9b87415eddf7549200))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(frontend): edit a connection's config and schedule
  ([`df75908`](https://github.com/thiesgerken/ferret/commit/df759081ca024c84a04f1ba08b82a07691022615))

  Fixing a typo in a connection no longer means deleting it, which would wipe its documents. The edit form starts from the stored config and schedule. Fields left empty fall back to the plugin's defaults, as in the create form. Edit is off while no worker offers the plugin, since the server can't validate the config then.

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(frontend): manage connections
  ([`ea4ddec`](https://github.com/thiesgerken/ferret/commit/ea4ddec704d042720529c35b108b08ae911c78d5))

  The Connections page creates a connection with a form rendered from the plugin's config schema (strings, lists of strings, booleans; anything else fails visibly), shows each connection's last and next run, runs it now, disables it and deletes it once its source is typed. The run history shows each run's stats, error and log. While a run is due or running, the page polls.

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(plugins): the worker loop and a git plugin
  ([`620603e`](https://github.com/thiesgerken/ferret/commit/620603ef91732582157c4ae58af0856dd843ef0c))

  ferret-plugins registers its plugins with their config schemas, claims runs by long polling, sends heartbeats with the run's log lines (and stops a run the server ended), and reports the result. The git plugin clones the configured branch, indexes every included file as a document (Markdown by glob, title from the first heading, author and dates from the history, optional deep link with {path} and {commit}) and sweeps removed files, all inside one sync per run. The plugins image gets git.

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): connections, plugin runs and the internal worker API
  ([`13784a1`](https://github.com/thiesgerken/ferret/commit/13784a10f1f487ca21b9689f65fb96dcbfb5486d))

  Workers register their plugins with a config schema and claim due runs from the internal API on port 8081 (worker PSK), by long polling: the claim waits until the next connection is due, a run lease runs out, or a NOTIFY says a connection became due (run now, new or changed connection). `FOR UPDATE SKIP LOCKED` and a partial unique index keep it at one live run per connection. A claimed run carries a token for the public API, ingest:write below the connection's root, valid while the run is live. Heartbeats carry log lines; a run without one for two minutes is lost. Only a successful run stores its state.

  Connections are managed over GraphQL: create (config validated against the plugin's schema, cron schedule), update, run now, delete with the typed source root (takes the documents below it), and the run history. Connection roots can't overlap, and API keys can't write below them.

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(frontend): the Explore page
  ([`b6e95cf`](https://github.com/thiesgerken/ferret/commit/b6e95cfb8a36c9a909b2340b3021a48e259505c2))

  /explore lists the user's documents in a sortable table with facet lists beside it. Rows open the shared viewer, and the state lives in the URL like search's.

  Part of #17

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): explore documents with facet counts
  ([`0023237`](https://github.com/thiesgerken/ferret/commit/0023237466ca434f8caafa8effb0366b73f74dce))

  GraphQL `explore` returns a sorted page of the user's documents, their total, and counts per source, content type, author (top 20) and month. The filters mean what search's mean (source prefix, content type, any author, created_at range). Each facet's counts apply every filter but its own, so they show what picking another value would give.

  Part of #17

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat: search results and the document viewer
  ([`794c2e1`](https://github.com/thiesgerken/ferret/commit/794c2e1371f82d4b7430b3ae869b4f14cc3f1854))

- ✨ feat(ui): search results with filter chips, and the document viewer
  ([`4cb9933`](https://github.com/thiesgerken/ferret/commit/4cb9933849f87025fbe4fc31146c70306a8c5fac))

  A query moves the search field to the top: one hit per document with its highlighted snippet, chips for time range, the hits' sources and authors, and a link to the origin. The state lives in the URL. A hit opens /view?id=…&start=…&end=…, a query-param route because the static export has no page per document. The viewer renders Markdown (images as links, so nothing loads from remote servers), plain text and email headers, and scrolls to the marked hit.

  Part of #5

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): search and read documents over GraphQL, filter by author
  ([`505eba3`](https://github.com/thiesgerken/ferret/commit/505eba37434f5d8503b8101351045561e35debe6))

  GraphQL `search` and `document` call the same service functions as `GET /api/search` and `GET /api/documents/{id}`, so both APIs return the same hits. Search gains the author filter from the conception (`author=`, any of them matches), applied before the candidate limit so a rare author doesn't lose hits.

  Part of #5

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(ui): map hit offsets into document text
  ([`bbd629c`](https://github.com/thiesgerken/ferret/commit/bbd629cb13abedea20dbfe83eec4516e527c25bc))

  The server counts characters, JavaScript indexes UTF-16 code units. The viewer marks the Markdown blocks that overlap a hit.

  Part of #5

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): syncs and sweeps
  ([`c79b59c`](https://github.com/thiesgerken/ferret/commit/c79b59c01e97c769e9fd31d5e32e0f9eedbe71e2))

- ✨ feat(server): ingest documents and find them by keyword
  ([`c6931e1`](https://github.com/thiesgerken/ferret/commit/c6931e1d26b53f23c8c9676a72a6634554a27f02))

- ✨ feat(server): syncs that sweep what the origin no longer has
  ([`677d825`](https://github.com/thiesgerken/ferret/commit/677d8255b2b6880d7620dadef4a1fc1e19dfac04))

  POST /api/syncs starts a sync over a source, `seen` reports unchanged documents without their content, and the commit deletes the documents below the sync's source that were neither put nor seen since it started. Without a commit nothing is deleted. `seen` names its source, since a sync over a root covers sub-sources whose external ids may repeat. Syncs need ingest:write within the key's sources; a committed sync answers 409.

  Part of #7

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat: API keys
  ([`7e54580`](https://github.com/thiesgerken/ferret/commit/7e545801365b7c10577ec5e515e0eded502f6d2a))

- ✨ feat(ui): sign in and out
  ([`91e5778`](https://github.com/thiesgerken/ferret/commit/91e5778dad587b237768e63d2c95cfbdf7dda80b))

- ✨ feat(server): ingest documents and find them by keyword
  ([`fdc1923`](https://github.com/thiesgerken/ferret/commit/fdc19238e5fa8999e79fa445c7dfa3671d9cfc60))

  PUT, DELETE and GET /api/documents and GET /api/search. Documents are chunked server-side (Markdown by heading, plain text by paragraph), each chunk indexed German- and English-stemmed and as trigrams, and search returns one hit per document with its best chunk highlighted. API keys' source restrictions apply to every route, in SQL for search.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(server): sign in and out
  ([`448e58f`](https://github.com/thiesgerken/ferret/commit/448e58f14845e7cae6304b6976be5b9b04824ff2))

- ✨ feat(ui): manage API keys in Settings
  ([`26c084f`](https://github.com/thiesgerken/ferret/commit/26c084f514a332b053267fdf24724cede49efbfd))

  Settings lists the signed-in user's keys with their scopes, sources, last use and expiry, creates keys (one access level per scope, admin scopes for admins only) and shows a new key once, and revokes keys.

  Part of #3

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(chart): add the base Helm chart
  ([`3299752`](https://github.com/thiesgerken/ferret/commit/32997522b6796b82d0d5138d62ddea766f9b1a80))

- ✨ feat(chart): route /auth to the server
  ([`2b75834`](https://github.com/thiesgerken/ferret/commit/2b7583485268b522c82d658fb0d5e83171adf68f))

  The session endpoints live under /auth, next to /graphql and /api.

  Part of #19

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): API keys for the REST API
  ([`ce33521`](https://github.com/thiesgerken/ferret/commit/ce3352191da3adfc85c0c6ff2c2f345ff4a577bb))

  Keys look like fk_<8 hex>.<secret>; only a sha256 of the secret is stored, and validation hashes before the prefix lookup and compares in constant time. Scopes follow <scope>:<access> with write implying read, admin scopes only work while the owner is an admin, and an optional source list restricts every scope. GET /api/users (admin:read) is the first bearer route. GraphQL creates, lists and revokes the signed-in user's keys.

  Part of #3

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✨ feat(server): sign in and out, bootstrap admin, GraphQL me
  ([`90eb212`](https://github.com/thiesgerken/ferret/commit/90eb2129ee802d57663b23922f36a8e8f2ca6c2b))

  Users and sessions tables, the bootstrap admin from FERRET_BOOTSTRAP_PASSWORD, POST /auth/login and /auth/logout with a 14-day HS256 session cookie checked against its session row on every request, and POST /graphql (JSON only) with `me`. `ferret schema` prints the SDL into frontend/schema.graphql, CI fails when it's stale. Queries go through sqlx's checked macros with offline metadata in .sqlx.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✨ feat(ui): add the frontend skeleton
  ([`6ac4989`](https://github.com/thiesgerken/ferret/commit/6ac49894f8f79295741d9640caa5a21c98ac914f))

- ✨ feat: walking skeleton (backend, compose, test harness, CI)
  ([`0084875`](https://github.com/thiesgerken/ferret/commit/008487516a9a69ad83197cd1f4aca627630b2a67))

### Other


- Merge pull request #91 from thiesgerken/viewer-frontmatter
  ([`247a984`](https://github.com/thiesgerken/ferret/commit/247a984428b80afd2695753333d6e1abe3b83e43))

- Merge remote-tracking branch 'origin/integration/v1' into 84-search-facets
  ([`4812a84`](https://github.com/thiesgerken/ferret/commit/4812a84b948e04f498f04fcac09308416fc097f8))

- Merge pull request #90 from thiesgerken/84-home
  ([`38bd11f`](https://github.com/thiesgerken/ferret/commit/38bd11fafaf543d3e57a16a7027b153254c8c5a2))

- Merge pull request #89 from thiesgerken/84-filter-column
  ([`6d68000`](https://github.com/thiesgerken/ferret/commit/6d68000efa324055f2f28b8dc518857d94c5dd82))

- Merge pull request #86 from thiesgerken/84-hit-layout
  ([`bec1009`](https://github.com/thiesgerken/ferret/commit/bec1009e66d506e517a30fcd15df1d15c4b59834))

- Merge pull request #83 from thiesgerken/82-typing-router
  ([`0f69a17`](https://github.com/thiesgerken/ferret/commit/0f69a176f599e021176ea05c601473d2e88a59a9))

- Merge pull request #81 from thiesgerken/72-calibrate-cut-off
  ([`329ae83`](https://github.com/thiesgerken/ferret/commit/329ae838aafdf6a9cf9801b571a7fef0b2aca5ce))

- Merge pull request #80 from thiesgerken/tei-bulk
  ([`135557a`](https://github.com/thiesgerken/ferret/commit/135557acea43f5fe9320b16fdeb135311443f2b7))

- Merge pull request #79 from thiesgerken/carapace-files-skip
  ([`b7d3797`](https://github.com/thiesgerken/ferret/commit/b7d37978ebae458d5414badcc0bcb8483aacb515))

- Merge pull request #76 from thiesgerken/72-search-ui-2
  ([`b9bab9f`](https://github.com/thiesgerken/ferret/commit/b9bab9f76d180ab294c95fcb2c44ecb8a0e0da30))

- Merge pull request #75 from thiesgerken/72-search-stats-2
  ([`4424624`](https://github.com/thiesgerken/ferret/commit/4424624a90a69d2b4c5c5ed49a069fba8e01e862))

- 📝 docs(search): link the cut-off section and rewrap the hybrid paragraph
  ([`72e3e95`](https://github.com/thiesgerken/ferret/commit/72e3e9586dae3438a0a9d73acedd288e5e8bb40c))

  Part of #72.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 📝 docs(search): the similarity cut-off, what null scores and the lower bound mean
  ([`09230ae`](https://github.com/thiesgerken/ferret/commit/09230ae14aa37821e8f8a857266483c9cc6eb479))

  Part of #72.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge pull request #71 from thiesgerken/embed-livelock
  ([`5a061a3`](https://github.com/thiesgerken/ferret/commit/5a061a3b3ec08755a2515c3922a8b21d9ea6a47e))

- Merge pull request #69 from thiesgerken/tei-healthcheck
  ([`27daa61`](https://github.com/thiesgerken/ferret/commit/27daa617dc0d36114ceb9070ddc088c8ce6a80c0))

- Merge pull request #68 from thiesgerken/67-demo-seed
  ([`39e59ff`](https://github.com/thiesgerken/ferret/commit/39e59ffdd3e7a23fad59e9401ed91fee1dc3326c))

- Merge pull request #67 from thiesgerken/readme-polish
  ([`8b9007c`](https://github.com/thiesgerken/ferret/commit/8b9007c8b6cc1de0590539b0d2e35f92ad9c6b11))

  📝 docs(readme): centered mark, badges and emoji headings

- 📝 docs(readme): release badge
  ([`404dad0`](https://github.com/thiesgerken/ferret/commit/404dad005c3d59cdbd9c3350343ed7fe9019bf96))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs(readme): h1 title, decorative mark, no pinned version or duplicate doc links
  ([`4cce54a`](https://github.com/thiesgerken/ferret/commit/4cce54a10a9ee3cce27f9f65d98c288ccd38b15c))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs(readme): centered mark, badges and emoji headings
  ([`9cd0a60`](https://github.com/thiesgerken/ferret/commit/9cd0a6006e650911e6175fcf91978425c10c4830))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge pull request #66 from thiesgerken/22-brand-mark
  ([`a58357e`](https://github.com/thiesgerken/ferret/commit/a58357e1409212237c274bcda169ff7059829e76))

- Merge pull request #65 from thiesgerken/64-worker-liveness
  ([`76466b9`](https://github.com/thiesgerken/ferret/commit/76466b9e6d69bf758995bfe575367f9f985109e2))

- 📝 docs: a restarted worker shows as a new row
  ([`a574842`](https://github.com/thiesgerken/ferret/commit/a574842f8cd202331354d44bf47c9ff38702ae94))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge pull request #56 from thiesgerken/5-plain-snippets
  ([`81ca00d`](https://github.com/thiesgerken/ferret/commit/81ca00dad00cb04f71a907c6433c70e57ed72e69))

- Merge pull request #55 from thiesgerken/feat/54-own-password
  ([`2c51427`](https://github.com/thiesgerken/ferret/commit/2c51427fc6c3e67632352fc9ba9936fd662213c6))

- Merge pull request #42 from thiesgerken/17-explore
  ([`f17fba2`](https://github.com/thiesgerken/ferret/commit/f17fba219b1d0113dd8a14336769b94a55a6dc02))

- Merge remote-tracking branch 'origin/integration/v1' into 17-explore
  ([`4a23d1f`](https://github.com/thiesgerken/ferret/commit/4a23d1fb6d33db7a48a858dabdfe2575001c431d))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	docs/conception/architecture.md

- Merge pull request #53 from thiesgerken/docs/22-consistency-pass
  ([`7dc376f`](https://github.com/thiesgerken/ferret/commit/7dc376fae76915e6c5b4f77533625cae922e89f8))

  📝 docs: align the conception docs with what v1 built

- 📝 docs: meta filters and per-card counts post-v1, exclusions on the card, review nits
  ([`dd4c6e0`](https://github.com/thiesgerken/ferret/commit/dd4c6e0e1d1ba18212d5088fb4cd5ab123a0d062))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge remote-tracking branch 'origin/integration/v1' into docs/22-consistency-pass
  ([`a731c93`](https://github.com/thiesgerken/ferret/commit/a731c9300872d496f9faa4dcf9833dafc2dc7e2c))

  # Conflicts: #	docs/conception/auth.md

- 📝 docs: align the conception docs with what v1 built
  ([`f7dde1f`](https://github.com/thiesgerken/ferret/commit/f7dde1f1761fcbbe01fcfe5ba6a98f3313c5a380))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge pull request #52 from thiesgerken/13-carapace-incremental
  ([`3bb7ca1`](https://github.com/thiesgerken/ferret/commit/3bb7ca16eb92eb2b58995630607cfbfefd364333))

- 💚 fix(test): amend the commit with an identity, which CI's git lacks
  ([`61c573b`](https://github.com/thiesgerken/ferret/commit/61c573b2e274f2c9bcf0f206af91e8f308ed0c39))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ✅ test(plugins): carapace moves between months and falls back to full syncs
  ([`2ffe86a`](https://github.com/thiesgerken/ferret/commit/2ffe86af7ef7c333b89916ea0651d9e42ad321f0))

  - A session moving to another month's directory keeps its documents (put before delete).
  - An unparsable version at the cursor and a force push both fall back to a full sync.
  - Docs: a document comes from one path only.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/integration/v1' into 13-carapace-incremental
  ([`ce170a5`](https://github.com/thiesgerken/ferret/commit/ce170a53cd5b35f2fc8960d93bb327c855fb0bee))

  # Conflicts: #	crates/ferret-plugins/src/plugins/carapace/mod.rs

- Merge pull request #51 from thiesgerken/18-admin
  ([`8f5724b`](https://github.com/thiesgerken/ferret/commit/8f5724bf19b44a8f1462c432c2a58319b44c21fe))

- Merge remote-tracking branch 'origin/integration/v1' into otto/18-admin
  ([`8545f13`](https://github.com/thiesgerken/ferret/commit/8545f13fb92e031c20e20b6ef04a3df44bd79b48))

  # Conflicts: #	crates/ferret-server/src/graphql.rs

- Merge remote-tracking branch 'origin/integration/v1' into 18-admin
  ([`e92d089`](https://github.com/thiesgerken/ferret/commit/e92d0896accca6af6f16ab6f2ccdc5aee7f57a08))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge remote-tracking branch 'origin/integration/v1' into 18-admin
  ([`8e2f6f6`](https://github.com/thiesgerken/ferret/commit/8e2f6f6e662773a9040e570224352d2c81bf749a))

- Merge remote-tracking branch 'origin/integration/v1' into 18-admin
  ([`29bd186`](https://github.com/thiesgerken/ferret/commit/29bd186396052c795a4a589f1e9c78c116f2304c))

  # Conflicts: #	crates/ferret-server/src/graphql.rs

- Merge remote-tracking branch 'origin/integration/v1' into 17-explore
  ([`7df9015`](https://github.com/thiesgerken/ferret/commit/7df901580fe8cda42bc8eeaaaa536e16803e1149))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	crates/ferret-server/src/lib.rs #	frontend/schema.graphql

- Merge pull request #44 from thiesgerken/14-exclusions
  ([`673568d`](https://github.com/thiesgerken/ferret/commit/673568dfa0b02232c8ddce60bcb6173c63a9c5e8))

- Merge remote-tracking branch 'origin/integration/v1' into 14-exclusions
  ([`fe0d05e`](https://github.com/thiesgerken/ferret/commit/fe0d05e1e3538cf0e9ac8765ea54e8a44e2eaf1f))

- Merge remote-tracking branch 'origin/integration/v1' into 14-exclusions
  ([`fb97fba`](https://github.com/thiesgerken/ferret/commit/fb97fba33334b083b6214d3e90b031ebad27f4c8))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	frontend/schema.graphql

- Merge remote-tracking branch 'origin/integration/v1' into 14-exclusions
  ([`3c421c6`](https://github.com/thiesgerken/ferret/commit/3c421c60cb199657f3b679fd9b20b1c81dddcf7b))

  # Conflicts: #	crates/ferret-plugins/src/plugins/git.rs #	crates/ferret-server/src/graphql.rs #	frontend/src/components/document-viewer.test.tsx #	frontend/src/components/document-viewer.tsx

- Merge remote-tracking branch 'origin/8-threads' into 14-exclusions
  ([`1bac09f`](https://github.com/thiesgerken/ferret/commit/1bac09f1e48d23a186ad3af6337c85587f949f7a))

- Merge pull request #50 from thiesgerken/feat/19-gpu-tei
  ([`da87fc6`](https://github.com/thiesgerken/ferret/commit/da87fc6eae7e7e4f575941a157c45ad967c414c2))

- Merge remote-tracking branch 'origin/integration/v1' into 17-explore
  ([`99c5d8e`](https://github.com/thiesgerken/ferret/commit/99c5d8ec435fa2a0c4534f87fe83f98617de209c))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	frontend/schema.graphql

- Merge pull request #43 from thiesgerken/12-paperless
  ([`c9d2602`](https://github.com/thiesgerken/ferret/commit/c9d2602b414cc6d4569aa54f75e450278304fd64))

- Merge remote-tracking branch 'origin/integration/v1' into 12-paperless
  ([`7624ec5`](https://github.com/thiesgerken/ferret/commit/7624ec5bfec5758c5e27843660b5dbd55667171a))

  # Conflicts: #	crates/ferret-plugins/src/plugins/mod.rs

- Merge remote-tracking branch 'origin/11-plugin-credentials' into 12-paperless
  ([`b1ad06c`](https://github.com/thiesgerken/ferret/commit/b1ad06cccfcc006f6b65fa9726a79d8d33e999f6))

  # Conflicts: #	crates/ferret-plugins/src/plugins/mod.rs

- Merge pull request #49 from thiesgerken/feat/16-default-model
  ([`eacc3ea`](https://github.com/thiesgerken/ferret/commit/eacc3ea3eb989b44a5e45568a8a087a77415293e))

- Merge remote-tracking branch 'origin/integration/v1' into feat/16-default-model
  ([`7f95339`](https://github.com/thiesgerken/ferret/commit/7f95339de72119632dc66c555564f482918f749c))

  # Conflicts: #	crates/ferret-api/src/lib.rs #	crates/ferret-server/src/graphql.rs #	docs/conception/README.md #	frontend/schema.graphql

- Merge remote-tracking branch 'origin/integration/v1' into feat/16-default-model
  ([`3a3e5bd`](https://github.com/thiesgerken/ferret/commit/3a3e5bd6dc017504cd38b1640b9552792dc43381))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	crates/ferret-server/src/search.rs #	frontend/src/components/search-results.test.tsx #	frontend/src/components/search-results.tsx

- Merge remote-tracking branch 'origin/integration/v1' into feat/16-default-model
  ([`f843e6e`](https://github.com/thiesgerken/ferret/commit/f843e6e09ec86edc07f5bbb7fd30b195579927b9))

- 🔀 merge(integration): bring in #36 and #38, renumber the migration after #41's
  ([`6e85654`](https://github.com/thiesgerken/ferret/commit/6e856548fceb991c6f7d4f61d6a7ce1d4b6b9a5c))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge remote-tracking branch 'origin/integration/v1' into feat/16-default-model
  ([`09da6f1`](https://github.com/thiesgerken/ferret/commit/09da6f10bf2f487ddf3e193b55b603236c011e29))

- Merge remote-tracking branch 'origin/integration/v1' into 17-explore
  ([`c350713`](https://github.com/thiesgerken/ferret/commit/c3507136ca766e61e5c924efa2c0cb0ab720e511))

- Merge pull request #41 from thiesgerken/8-threads
  ([`0082cc1`](https://github.com/thiesgerken/ferret/commit/0082cc1ce9bac5dce870a22bff6a832a54c30497))

- ✅ test(server): k counts threads; stable thread representatives
  ([`0023e03`](https://github.com/thiesgerken/ferret/commit/0023e0353bcc43eacc22b2f9dde53061f0638959))

  - A test with k below a thread's size: the thread still yields one hit with collapsed = 2.
  - Equal scores within a thread break ties by created_at, id.
  - The search result and the viewer share one viewerHref.
  - Docs: the thread index and the thread order match the migration and the code.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/integration/v1' into 8-threads
  ([`ab18547`](https://github.com/thiesgerken/ferret/commit/ab18547da3621a3927de5297d6be5e9bf3615e12))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	frontend/src/components/search-results.test.tsx #	frontend/src/components/search-results.tsx

- Merge remote-tracking branch 'origin/integration/v1' into 8-threads
  ([`818097e`](https://github.com/thiesgerken/ferret/commit/818097e6d9490e695cbc0287682dd3ec0cc78538))

  Collapsing now runs on top of hybrid search's fused ranking. The thread count gets a window of its own without an order: with one, count(*) counts only up to the current row. That went unnoticed while the thread's documents tied on keyword scores.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/integration/v1' into 8-threads
  ([`9073b4f`](https://github.com/thiesgerken/ferret/commit/9073b4fa27b5d2de8055746a67198ec061e79c9e))

- Merge pull request #46 from thiesgerken/13-carapace
  ([`0a41d62`](https://github.com/thiesgerken/ferret/commit/0a41d62d2a66b410e2cbc4bd58e5f971733e4b06))

- Merge remote-tracking branch 'origin/11-plugin-credentials' into 13-carapace
  ([`0a9d997`](https://github.com/thiesgerken/ferret/commit/0a9d997c9d6fb3f408d79461f444280e976ad45d))

  The carapace plugin now clones and reads files through #10's repo::Checkout. Its own extraction from git.rs goes away.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/integration/v1' into 17-explore
  ([`82ceefb`](https://github.com/thiesgerken/ferret/commit/82ceefbcf097f77cdd1b5d2cb8dee224a8fa3a9b))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	frontend/schema.graphql

- Merge pull request #48 from thiesgerken/feat/37-titles-searchable
  ([`f6c3f90`](https://github.com/thiesgerken/ferret/commit/f6c3f900c705bce008e3cbd3974e08385a5bbb0d))

- Merge remote-tracking branch 'origin/integration/v1' into feat/37-titles-searchable
  ([`1a3a7a2`](https://github.com/thiesgerken/ferret/commit/1a3a7a2ba9dc9eaed195d4d382e2f97e7c3b9997))

- Merge remote-tracking branch 'origin/integration/v1' into feat/37-titles-searchable
  ([`9b27317`](https://github.com/thiesgerken/ferret/commit/9b273172edf98541284523796894d77a35af5d79))

- Merge pull request #47 from thiesgerken/feat/15-judgments
  ([`1cea45a`](https://github.com/thiesgerken/ferret/commit/1cea45a646a53e14f348e89e07900299b81fb14f))

- Merge remote-tracking branch 'origin/integration/v1' into feat/15-judgments
  ([`6c38cff`](https://github.com/thiesgerken/ferret/commit/6c38cff5c497a7654a01347c07dbce6d317366ca))

  # Conflicts: #	frontend/src/components/search-results.tsx

- Merge remote-tracking branch 'origin/integration/v1' into feat/15-judgments
  ([`effc9d6`](https://github.com/thiesgerken/ferret/commit/effc9d66e3b1393c5570c6163939ceed85c0f939))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	crates/ferret-server/src/lib.rs #	frontend/schema.graphql

- Merge pull request #39 from thiesgerken/11-plugin-credentials
  ([`48bb6a3`](https://github.com/thiesgerken/ferret/commit/48bb6a34283c3f8403f7ac758cbf7595e18c7fda))

- Merge branch '9-edit-connections' into 11-plugin-credentials
  ([`624af6e`](https://github.com/thiesgerken/ferret/commit/624af6e1b56d08bd6d7e74b4c1c30088809499d2))

  The git token moves into repo::Checkout::clone, the one place a clone is made since #10.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge branch '9-edit-connections' into 11-plugin-credentials
  ([`a09d766`](https://github.com/thiesgerken/ferret/commit/a09d766ea2f964176ae24766e5a634a4fec20689))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	crates/ferret-server/src/lib.rs #	crates/ferret-server/src/main.rs #	crates/ferret-server/tests/common/mod.rs #	docker-compose.yml

- Merge branch '9-edit-connections' into 11-plugin-credentials
  ([`650ec69`](https://github.com/thiesgerken/ferret/commit/650ec699fcbd3cc1c0ee231d2e2aca880ae11645))

  # Conflicts: #	crates/ferret-server/src/graphql.rs

- Merge branch '9-edit-connections' into 11-plugin-credentials
  ([`7569a39`](https://github.com/thiesgerken/ferret/commit/7569a39bd71cadb37518e6a4a92e0f58f2275347))

  # Conflicts: #	crates/ferret-plugins/src/plugins/git.rs #	crates/ferret-server/tests/runs.rs

- Merge pull request #36 from thiesgerken/9-edit-connections
  ([`0a15712`](https://github.com/thiesgerken/ferret/commit/0a1571210bba2de2cbf1422dad0dd5b4b960a140))

- Merge remote-tracking branch 'origin/integration/v1' into 9-edit-connections
  ([`7176568`](https://github.com/thiesgerken/ferret/commit/717656838544bdc5044c82a2aba5fea0ff9eb0f9))

- Merge pull request #38 from thiesgerken/33-linkable-urls
  ([`07197c7`](https://github.com/thiesgerken/ferret/commit/07197c76ce5010663573e26c25adfbc838fbab89))

- Merge pull request #45 from thiesgerken/feat/10-incremental-git
  ([`b117704`](https://github.com/thiesgerken/ferret/commit/b117704305e190e9d52090699bdbfe35d276af48))

- 🎨 refactor(plugins): honest full-sync reason, fail on untracked diff paths
  ([`53a03aa`](https://github.com/thiesgerken/ferret/commit/53a03aac356cd25a94b58f50efe3a83f6475092c))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 💚 fix(tests): amend with an explicit git identity, CI has none
  ([`25678f5`](https://github.com/thiesgerken/ferret/commit/25678f51fb979192169be938fd233c105dad9787))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✅ test(plugins): cover incremental git runs and their full-sync fallbacks
  ([`01661ab`](https://github.com/thiesgerken/ferret/commit/01661ab039b5e385d16105a6d98e3aabdda853e4))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge remote-tracking branch 'origin/integration/v1' into 9-edit-connections
  ([`c2fefee`](https://github.com/thiesgerken/ferret/commit/c2fefee38ff032d7e483c0c584c2d98a8a272916))

- Merge pull request #40 from thiesgerken/feat/6-semantic-hybrid-search
  ([`2333bb9`](https://github.com/thiesgerken/ferret/commit/2333bb91b05dc131ed93ac5670dc44f4cbfc292f))

- 🔀 merge(integration): bring in plugin workers
  ([`3bdfa96`](https://github.com/thiesgerken/ferret/commit/3bdfa967dda72d45c19708a96eb2a0dfa19577ff))

  Also lets a second pump claim chunks: the claim query no longer sorts after taking the advisory locks.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge pull request #35 from thiesgerken/9-connections
  ([`e52bddb`](https://github.com/thiesgerken/ferret/commit/e52bddbe41d8361f7cf9056ceb09b69ab3a4fdcf))

- Merge remote-tracking branch 'origin/integration/v1' into feat/6-semantic-hybrid-search
  ([`5ed2e2f`](https://github.com/thiesgerken/ferret/commit/5ed2e2f0cc3ed3d43f2e7ea2af73a031b2156e12))

  # Conflicts: #	crates/ferret-server/src/error.rs #	crates/ferret-server/src/lib.rs #	crates/ferret-server/src/search.rs

- 📝 docs: model config file, probe dimension, pump errors, hybrid ranking
  ([`f376318`](https://github.com/thiesgerken/ferret/commit/f3763183cb6d5586f18e79a17c72a50ca4bce4c1))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✅ test(server): fake TEI and semantic search tests
  ([`5545185`](https://github.com/thiesgerken/ferret/commit/5545185a4144c9da7143c17d8d5ca5eba5e4e254))

  Wait for TEI at startup instead of crashing while it downloads its model.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge remote-tracking branch 'origin/9-connections' into 9-edit-connections
  ([`bf3eed8`](https://github.com/thiesgerken/ferret/commit/bf3eed8d2a06f66c1ee4986c7e5954873b63bf64))

- Merge remote-tracking branch 'origin/integration/v1' into 9-connections
  ([`27fa0e9`](https://github.com/thiesgerken/ferret/commit/27fa0e9b89351db6764b94a98e9bee59a26d2bc2))

  # Conflicts: #	crates/ferret-server/src/graphql.rs #	frontend/schema.graphql

- Merge remote-tracking branch 'origin/9-connections' into 9-edit-connections
  ([`8fa8d39`](https://github.com/thiesgerken/ferret/commit/8fa8d3909cde244fb82d3ec329ecd4743e3974fe))

  # Conflicts: #	frontend/src/components/connections.tsx

- 📝 docs: run tokens, worker environment and the git plugin as built
  ([`7fb9e8a`](https://github.com/thiesgerken/ferret/commit/7fb9e8a6283326bd306a12d90a1c41abc10f4d61))

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/integration/v1' into 9-connections
  ([`2bd6782`](https://github.com/thiesgerken/ferret/commit/2bd6782f1ad309b6733c59fc9ed6f037ef96e7a3))

- Merge remote-tracking branch 'origin/7-syncs' into 9-connections
  ([`5606a7f`](https://github.com/thiesgerken/ferret/commit/5606a7f5bcbc8bce5613b767c6240d6fa06a48b0))

  # Conflicts: #	crates/ferret-server/src/documents.rs #	crates/ferret-server/src/syncs.rs

- Merge pull request #34 from thiesgerken/fix/33-url-schemes
  ([`bff3463`](https://github.com/thiesgerken/ferret/commit/bff34637b34a2911047abad8f97cbde27b6c1fc5))

- Merge pull request #31 from thiesgerken/5-search-ui
  ([`794c2e1`](https://github.com/thiesgerken/ferret/commit/794c2e1371f82d4b7430b3ae869b4f14cc3f1854))

- ✅ test(server): compare every hit field between GraphQL and REST search
  ([`cd5b687`](https://github.com/thiesgerken/ferret/commit/cd5b687a23919acf083e25597a4d80a9ab68f8b9))

  The parity test now also compares content type, title, url, thread and both timestamps; the documents set them so the comparison isn't all nulls. The README's Search section says which timestamp the hit shows and that `author=` repeats.

  Part of #5

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/integration/v1' into 5-search-ui
  ([`5a6e4d7`](https://github.com/thiesgerken/ferret/commit/5a6e4d75c98ebf415b29723b7645f2de095d934f))

  GraphQL search takes contentTypes like the REST search; the author filter becomes a pushed clause in the built candidate query.

- 📝 docs: routes with ids use query parameters
  ([`2220acb`](https://github.com/thiesgerken/ferret/commit/2220acb55be1a278562c3d769838aa5d0a5c964c))

  Part of #5

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/feat/4-ingest-keyword-search' into 5-search-ui
  ([`5db463a`](https://github.com/thiesgerken/ferret/commit/5db463af7efb4f5ae9adce56433d4ac86a5eb9b2))

- Merge pull request #32 from thiesgerken/7-syncs
  ([`c79b59c`](https://github.com/thiesgerken/ferret/commit/c79b59c01e97c769e9fd31d5e32e0f9eedbe71e2))

- ✅ test(server): a sweep leaves other users' documents alone
  ([`13290ad`](https://github.com/thiesgerken/ferret/commit/13290ad8feb2484ee16a51f0eadb6e9f78f10ddd))

  The source check moves onto ApiCaller as `require_source`, next to `require`, now that documents and syncs both use it. The README notes `seen`'s 409.

  Part of #7

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/integration/v1' into 7-syncs
  ([`ca50d7b`](https://github.com/thiesgerken/ferret/commit/ca50d7bf31c7816348dc36564debe626717397e8))

- Merge pull request #30 from thiesgerken/feat/4-ingest-keyword-search
  ([`c6931e1`](https://github.com/thiesgerken/ferret/commit/c6931e1d26b53f23c8c9676a72a6634554a27f02))

- 📝 docs: per-word matching, substring phrases, character cap in embeddings.md
  ([`254a8bb`](https://github.com/thiesgerken/ferret/commit/254a8bbb94ff5f763547ba5541c453f13119841c))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge remote-tracking branch 'origin/integration/v1' into feat/4-ingest-keyword-search
  ([`b9679ce`](https://github.com/thiesgerken/ferret/commit/b9679cef4db26547183e3785c928001e35590c16))

- Merge pull request #29 from thiesgerken/3-api-keys
  ([`7e54580`](https://github.com/thiesgerken/ferret/commit/7e545801365b7c10577ec5e515e0eded502f6d2a))

- Merge remote-tracking branch 'origin/3-api-keys' into feat/4-ingest-keyword-search
  ([`07db5ca`](https://github.com/thiesgerken/ferret/commit/07db5ca67b4926fc4fa2e90311362e4a83e1ff1b))

  # Conflicts: #	crates/ferret-server/src/api_keys.rs

- Merge remote-tracking branch 'origin/integration/v1' into 3-api-keys
  ([`cdda48f`](https://github.com/thiesgerken/ferret/commit/cdda48fcccdfb4d84bee7ca059e5e59189f95e16))

- Merge pull request #24 from thiesgerken/2-login-frontend
  ([`91e5778`](https://github.com/thiesgerken/ferret/commit/91e5778dad587b237768e63d2c95cfbdf7dda80b))

- Merge remote-tracking branch 'origin/integration/v1' into 2-login-frontend
  ([`8d92f3b`](https://github.com/thiesgerken/ferret/commit/8d92f3bfb2bbd99175b2d1f7a4d5f45074c40df4))

  # Conflicts: #	frontend/schema.graphql

- 📝 docs: ingest validation, character-based chunks, search semantics
  ([`aa53b5b`](https://github.com/thiesgerken/ferret/commit/aa53b5be876e91b06509740c56d2a2831dbccdd4))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✅ test(server): create API keys from the shared test harness
  ([`b83d3c4`](https://github.com/thiesgerken/ferret/commit/b83d3c4041bffe96352348c275b3b737f761696b))

  Every /api test from #4 on needs a key with given scopes and sources.

  Part of #3

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/integration/v1' into 3-api-keys
  ([`dcf2bc9`](https://github.com/thiesgerken/ferret/commit/dcf2bc9e33263194f07065e241de03dc05976a3a))

- Merge pull request #28 from thiesgerken/20-release-pipeline
  ([`5979fa1`](https://github.com/thiesgerken/ferret/commit/5979fa17271e84c2aa7d097fa82d3a04a7bdadc7))

- Merge remote-tracking branch 'origin/integration/v1' into 3-api-keys
  ([`0904b9b`](https://github.com/thiesgerken/ferret/commit/0904b9bb20afcc318771a0537fa55bb72fa6a0e8))

- Merge pull request #27 from thiesgerken/feat/2-login-backend
  ([`448e58f`](https://github.com/thiesgerken/ferret/commit/448e58f14845e7cae6304b6976be5b9b04824ff2))

- Merge remote-tracking branch 'origin/2-login-frontend' into 3-api-keys
  ([`abfa46d`](https://github.com/thiesgerken/ferret/commit/abfa46df72e9068c9ad5315848b502c42d0fdb19))

  # Conflicts: #	frontend/schema.graphql

- Merge remote-tracking branch 'origin/1-skeleton-frontend' into 2-login-frontend
  ([`37f0323`](https://github.com/thiesgerken/ferret/commit/37f03236a7cb0b694b1f477ca7f9af9fe97e2f0f))

- Show the login form when the session ends mid-use
  ([`d0feaf7`](https://github.com/thiesgerken/ferret/commit/d0feaf787399c3f6507928556884f03ce9645dbd))

  Any GraphQL 401 now swaps the page for the login form, not just the initial `me`. The session endpoints move to /auth, the menu closes on navigation and reads its user through a fragment.

  Part of #2

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Sign in and out in the frontend
  ([`efd34ac`](https://github.com/thiesgerken/ferret/commit/efd34acf43a15ad5a7e703601a06f1c39d753127))

  While signed out, every page renders the login form in its place, so a deep link survives the sign-in. Signed in, an app menu in the top right leads to the other sections (pages follow in later tickets) and signs out. Sign-in and sign-out reload the page, so nothing of the previous session stays in the GraphQL cache.

  The placeholder schema now holds the `me` contract agreed with the server side. GraphQL goes over POST only.

  Part of #2

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Merge remote-tracking branch 'origin/feat/2-login-backend' into 3-api-keys
  ([`1377a28`](https://github.com/thiesgerken/ferret/commit/1377a28fb5d56661dd36951d63a8bcdd2794e0cb))

  # Conflicts: #	Cargo.toml

- 📝 docs: scope the error shape, cookie at logout, bootstrap restore in the cluster
  ([`7f9fd15`](https://github.com/thiesgerken/ferret/commit/7f9fd154c8ae107ef8798e33ab17bce9f81425d7))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ✅ test(server): bootstrap also restores a demoted admin
  ([`d02302f`](https://github.com/thiesgerken/ferret/commit/d02302fef7a698a2cb879f6f823e92a9b8a19a12))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge remote-tracking branch 'origin/integration/v1' into feat/2-login-backend
  ([`9e82f72`](https://github.com/thiesgerken/ferret/commit/9e82f72c3004a0e1129f80e3bf960bd46dda2653))

  # Conflicts: #	docs/conception/architecture.md

- Merge pull request #26 from thiesgerken/19-helm-chart-base
  ([`3299752`](https://github.com/thiesgerken/ferret/commit/32997522b6796b82d0d5138d62ddea766f9b1a80))

- Merge remote-tracking branch 'origin/integration/v1' into 19-helm-chart-base
  ([`c552ffc`](https://github.com/thiesgerken/ferret/commit/c552ffc0a32efab2486b9cdd1ce97ee87395a78c))

  # Conflicts: #	.github/workflows/ci.yml

- Merge remote-tracking branch 'origin/integration/v1' into 19-helm-chart-base
  ([`f3180f5`](https://github.com/thiesgerken/ferret/commit/f3180f5a77549d3c78c0f4aa1bc074e26853f5ed))

- Add the base Helm chart
  ([`29ba7b0`](https://github.com/thiesgerken/ferret/commit/29ba7b0689ffc58b5e6d2dfd8110a8535d68609c))

  Server, frontend and ParadeDB with a PVC, an HTTPRoute that sends /graphql and /api to the server and everything else to the frontend, and one Secret holding the Postgres password, the bootstrap password and the session secret, generated once and kept via `lookup` or taken from an existing Secret. TEI, plugin workers and their secrets follow with #6, #9 and #11.

  Part of #19

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 📝 docs: session endpoints under /auth, CSRF rules, server secrets
  ([`3c549c8`](https://github.com/thiesgerken/ferret/commit/3c549c8d05886df9bcf5c3164533f69e6e8f3433))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Merge pull request #23 from thiesgerken/1-skeleton-frontend
  ([`6ac4989`](https://github.com/thiesgerken/ferret/commit/6ac49894f8f79295741d9640caa5a21c98ac914f))

- Merge remote-tracking branch 'origin/integration/v1' into 1-skeleton-frontend
  ([`098fc45`](https://github.com/thiesgerken/ferret/commit/098fc452503cf76185f09007a9bfd97fd2398aa6))

- Merge pull request #25 from thiesgerken/feat/1-skeleton-backend
  ([`0084875`](https://github.com/thiesgerken/ferret/commit/008487516a9a69ad83197cd1f4aca627630b2a67))

- 📝 docs: server environment and health probe, image naming
  ([`c5b543c`](https://github.com/thiesgerken/ferret/commit/c5b543c7d53516a2fc5990353beef9133cae1b84))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 📝 docs: add README and AGENTS.md
  ([`5225110`](https://github.com/thiesgerken/ferret/commit/5225110997be215228a4db6141f297f48f8c8c30))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Walking skeleton: Rust workspace, compose, test harness, CI
  ([`7186df0`](https://github.com/thiesgerken/ferret/commit/7186df0866ef2be6b7f358aae96d0c2a4e778361))

  Cargo workspace with ferret-server (binary `ferret`), ferret-plugins and ferret-api. The server applies its embedded migrations at startup, refuses to start on an edited one, and answers GET /api/health. Black-box tests run the server in-process against a per-test ParadeDB database.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- Proxy /auth to the server in development
  ([`c1c80f6`](https://github.com/thiesgerken/ferret/commit/c1c80f6a49ab44ee16ccec07478f12a0fc8c1908))

  The session endpoints live under /auth, next to /graphql and /api.

  Part of #1

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Record the pinned pnpm in the lockfile
  ([`62184cf`](https://github.com/thiesgerken/ferret/commit/62184cf67df969622f3b1339d2c2a0f25d163cf9))

  pnpm 12 locks the packageManager version alongside the dependencies.

  Part of #1

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Pin pnpm and drop two pass-through files
  ([`210ad53`](https://github.com/thiesgerken/ferret/commit/210ad533f3b06650b36407fcdcd0d3f7198e678f))

  corepack in the image and CI now runs the pnpm that wrote the lockfile. The theme provider wrapper only forwarded fixed props, and the CSS module declaration isn't needed for typechecking.

  Part of #1

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Add the frontend skeleton from carapace
  ([`190b73e`](https://github.com/thiesgerken/ferret/commit/190b73e97fe96cd61f87f8caa58d8e180eb2a6e7))

  Next.js static export with carapace's stack and look (Tailwind 4, Lucide, next-themes, the same color tokens), stripped to the empty search page: logo and a centered search field that submits `/?q=`. urql is wired as the GraphQL client, GraphQL Code Generator generates typed documents from schema.graphql (a placeholder until the server prints its SDL). `next dev` proxies `/graphql` and `/api` to the local server, the Docker image serves the export with nginx.

  Tests run with `node --test` against jsdom, which a preload installs before react-dom loads.

  Part of #1

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- Start v1 integration
  ([`e5a2f8a`](https://github.com/thiesgerken/ferret/commit/e5a2f8a015ac8993c7b26a82130c157170ecc06d))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Add ADRs for the core design decisions
  ([`81eedd2`](https://github.com/thiesgerken/ferret/commit/81eedd2e89487590ab27795224291b1723c629d1))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Define v1 scope
  ([`2331d51`](https://github.com/thiesgerken/ferret/commit/2331d517ac85a7d0b1c215ca99b1fb9d31e9eae8))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- README: carapace skill uses a source-restricted key
  ([`c0decc6`](https://github.com/thiesgerken/ferret/commit/c0decc68da5ecc3530f5b7bde72b5eb938816951))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Glossary: exclusion
  ([`5ed227c`](https://github.com/thiesgerken/ferret/commit/5ed227ca4bd0a4dd11808530897ece9908c7bc98))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Drop search log, add exclusions, source-scoped read access, bilingual stemming
  ([`d009b4e`](https://github.com/thiesgerken/ferret/commit/d009b4e9995d23089d3255318b2a1ff76b8d5bae))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- carapace plugin: concrete deep links
  ([`48e44bc`](https://github.com/thiesgerken/ferret/commit/48e44bc70cb8e985de1435bcac2eb5f950b11410))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Add glossary, connections, hit and time-filter semantics, backups via Velero
  ([`6ee9cfa`](https://github.com/thiesgerken/ferret/commit/6ee9cfab53597a8f901939904f1b1c8e01c47b5c))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Add email plugin conception and per-thread result collapsing
  ([`0f8c7ce`](https://github.com/thiesgerken/ferret/commit/0f8c7ce081df1687a8c3c9b0cafb62a05aa86dd0))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Decide default embedding model, add query prefixes and bulk endpoint
  ([`d24a44c`](https://github.com/thiesgerken/ferret/commit/d24a44ca1b7f763340c0256967a41c70cb5f0ed2))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- carapace plugin: parse Markdown mirror in v1, JSON sidecars in v2
  ([`d5c7abc`](https://github.com/thiesgerken/ferret/commit/d5c7abc0683cf23ea40f5007ecf4b53d4841fa7c))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Add plugins conception, hierarchical sources, path-free ingest routes
  ([`8131163`](https://github.com/thiesgerken/ferret/commit/81311630a7e750ff76b6a56e3676c5041449abf7))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Treat chats as plain documents segmented by the plugin, add thread field
  ([`5f1976b`](https://github.com/thiesgerken/ferret/commit/5f1976bb6ad4c039081ecdf35d065c0110945615))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Add embeddings conception: multiple live models, per-user evaluation
  ([`bd39c91`](https://github.com/thiesgerken/ferret/commit/bd39c91be263bb6c83b066a99ada731eed8ff9fa))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Add architecture conception: APIs, plugin workers, embedding, UI
  ([`c1fb043`](https://github.com/thiesgerken/ferret/commit/c1fb0434d5c9058bc5b467b1f137050ade27bb93))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Add auth conception: users, JWT sessions, scoped API keys
  ([`1f6e416`](https://github.com/thiesgerken/ferret/commit/1f6e41646fe571c241a07b61a1bd041f8802e6d0))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

- Add conception README and MIT license
  ([`066b272`](https://github.com/thiesgerken/ferret/commit/066b272b045661c73840e587bce6ca5e53149dda))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01VnwiAij2MLNkETBVvz6o1c

### 🐛 Bug Fixes


- 🐛 fix(ui): the viewer shows a Markdown frontmatter as a YAML block
  ([`247a984`](https://github.com/thiesgerken/ferret/commit/247a984428b80afd2695753333d6e1abe3b83e43))

- 🐛 fix(ui): the viewer shows a Markdown frontmatter as a YAML block
  ([`2104c38`](https://github.com/thiesgerken/ferret/commit/2104c384220e9c09b90590d0a58491a03f88f17e))

  Without frontmatter support, the closing `---` turned the YAML block of e.g. a carapace skill into a setext heading after a rule. With remark-frontmatter it renders as a code block at the top, and keeps its position so a hit inside it is marked like any other block.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(search): facets for parent roots and thread authors, one statement per run
  ([`0751212`](https://github.com/thiesgerken/ferret/commit/0751212b642060d9efaee0beb0201498681f013e))

  Part of #84.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(ui): every pause in typing searches, not just the first
  ([`0f69a17`](https://github.com/thiesgerken/ferret/commit/0f69a176f599e021176ea05c601473d2e88a59a9))

- 🐛 fix(ui): every pause in typing searches, not just the first
  ([`819f6fb`](https://github.com/thiesgerken/ferret/commit/819f6fbc748b8703594bec6c2dc4bfc19e419fae))

  Replacing the typing entry passed `history.state` as its data, which carries Next's `__NA` marker. Next's patched replaceState takes such a call for one of its own and doesn't tell the router, so the URL changed without a search. Only our own data goes in now.

  Closes #82

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(plugins): carapace indexes only Markdown as files
  ([`b7d3797`](https://github.com/thiesgerken/ferret/commit/b7d37978ebae458d5414badcc0bcb8483aacb515))

- 🐛 fix(plugins): carapace indexes only Markdown as files
  ([`b49e990`](https://github.com/thiesgerken/ferret/commit/b49e990b74bde965f5137408c8182bfa6a085706))

  Raw session dumps and skills' code crowded the keyword hits. The rule is part of the indexing settings, so the next run is a full sync that sweeps them. Fixes #78.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(search): an exhaustive vector half doesn't make the total a minimum
  ([`6d88087`](https://github.com/thiesgerken/ferret/commit/6d88087cc9f1bee968b8ba28d8529f122a8a39b7))

  All nearest chunks passing the cut-off only means more may exist while the scope holds more embedded chunks than came back. Part of #72.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(server): the embed pump shrinks its batch when TEI times out
  ([`5a061a3`](https://github.com/thiesgerken/ferret/commit/5a061a3b3ec08755a2515c3922a8b21d9ea6a47e))

- 🐛 fix(server): the embed pump shrinks its batch when TEI times out
  ([`a6631b2`](https://github.com/thiesgerken/ferret/commit/a6631b2a21dce8bba4d7cc517dc732726511ad95))

  A batch that times out keeps TEI busy, so retrying the same size on a slow CPU queued behind it and timed out forever. Fixes #70.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(deploy): probe TEI's /info, /health waits behind embedding batches
  ([`27daa61`](https://github.com/thiesgerken/ferret/commit/27daa617dc0d36114ceb9070ddc088c8ce6a80c0))

- 🐛 fix(server): the Admin page asks TEI's /info, a busy TEI isn't offline
  ([`4587fd3`](https://github.com/thiesgerken/ferret/commit/4587fd3400953d8fc4543a92f2177bccc31fe59a))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(deploy): probe TEI's /info, /health waits behind embedding batches
  ([`4b144dd`](https://github.com/thiesgerken/ferret/commit/4b144ddc60df0979124964202235231b6308ac60))

  TEI runs health checks on the backend thread that runs the batches, so /health times out while a backfill keeps TEI busy. Compose then marks it unhealthy and refuses to start the server, and in the chart the pod drops out of its Service.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(auth): password changes compare-and-swap the token version
  ([`f2b963e`](https://github.com/thiesgerken/ferret/commit/f2b963e0ad139951700dc551188c9b827e5df350))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(server): exclusions win races with PUTs and live runs
  ([`3abd0d3`](https://github.com/thiesgerken/ferret/commit/3abd0d3fb06cb6f72a594dbcbefa31875d133cef))

  - PUT holds a shared advisory lock per (user, source) from its exclusion check to its commit,
    and excluding takes it exclusively. A PUT racing an exclusion of its document (or a new member
    of an excluded thread) now sees the exclusion instead of recreating the document.
  - removeExclusion refuses while the connection has a running run, whose finish would overwrite
    the state reset. The connection's row lock keeps a run from being claimed meanwhile.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(plugins): stable Paperless listing, cursor overlap, public links
  ([`d60ba1d`](https://github.com/thiesgerken/ferret/commit/d60ba1dbb76863c44bd391872004c9c7e013f066))

  - Lists page `ordering=id`. A listing whose count changes or repeats an id fails the run
    instead of letting the sweep delete what it skipped.
  - The cursor overlaps by 10 minutes, for saves that commit after a listing.
  - `public_url` (optional) is what hits link to. The state stores both addresses, and a change
    fetches every document again.
  - A response that doesn't decode fails with a fixed message, not serde's quote of it.
  - Docs: plain-HTTP cluster URLs are accepted for this plugin, unlike git.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(plugins): pin Paperless API version 10 and refuse redirects
  ([`2e6634e`](https://github.com/thiesgerken/ferret/commit/2e6634ec84bfbf4c0c0cbf5c0309770844093aed))

  Paperless-ngx 3.3 accepts API versions 9 and 10, and 10 exists since 3.0 (checked against v3.3.2's source). The fields the plugin reads haven't changed shape. In 3.x, document versions stay out of the list, `content` is the latest version's, and a new version bumps the root's `modified`, so the cursor still sees it.

  A redirect now fails the run with a hint to use an address that reaches Paperless directly, such as the in-cluster service, instead of following it to a login proxy's page.

  Part of #12

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(search): keep the model select when the chosen model is gone
  ([`9d65c7c`](https://github.com/thiesgerken/ferret/commit/9d65c7cd9463b575a760f72c0432f2f921fee557))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(plugins): carapace tags with spaces, unreadable memory files, repeated facts
  ([`838eddd`](https://github.com/thiesgerken/ferret/commit/838eddd76806b1a64ab67b1ef76d50a9bfeeec0e))

  - Tags split between backticks, so `home lab` parses.
  - A session or digest file that can't be read fails the run instead of being skipped and swept.
  - A fact repeated within a session is indexed once, with a log line.
  - Query values keep RFC 3986's unreserved characters.
  - Docs: the "dated until" ambiguity and what created_at means.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(server): bound Explore's load, honest facet counts for selections
  ([`dc979d3`](https://github.com/thiesgerken/ferret/commit/dc979d3d390d122d24f8f0840a10e9b0589d0f49))

  - The six queries run one after another on one connection, so a page view holds one connection.
  - An index for the default sort: (user_id, created_at desc, id).
  - Selected authors are counted beyond the top 20, and a selected parent source counts every
    document it matches.
  - Dates render in UTC, like the month facet buckets them.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(embeddings): cap embedded titles, retry failures on a new title
  ([`9da0878`](https://github.com/thiesgerken/ferret/commit/9da08780c27edddc349cd5b9dd70e01cc308ef81))

  Also renumbers the migration after #39's and documents how to pick migration versions.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(frontend): the edit form never lacks its plugin's schema
  ([`684abf3`](https://github.com/thiesgerken/ferret/commit/684abf36a5d46a25e636b5e511517f9ccc0d1ac5))

  A connection's plugin type always exists, since connections reference plugin_types and those rows are never deleted. So the Edit button no longer has a gate that couldn't fire. A missing type is now an error. A new test edits a disabled connection, so `enabled` is checked to come from the connection.

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(embeddings): survive rejected chunks and stuck TEI, harden chart
  ([`2f024a3`](https://github.com/thiesgerken/ferret/commit/2f024a3ee6156fd3cb71451a6134730524210bd8))

  - The pump claims chunks with advisory locks instead of row locks, retries a
    rejected batch text by text and records the culprits in
    embedding_failures, so one bad chunk no longer stalls a model.
  - Timeouts on every TEI call: 5 s for queries (then keyword-only), 60 s per
    pump batch, 5 s to connect.
  - Model names must not collide once `.`, `_` and `-` are mapped, end in a
    letter or digit and stay within 40 characters.
  - No `distance < 1` cut: the vector half always contributes its top 100.
  - FERRET_CONFIG is now FERRET_CONFIG_FILE.
  - Chart: startup probe outlasts the 10-minute TEI wait, required model
    fields, validated and unique TEI names, amd64 node selector, cache size
    limit.
  - Tests: start_server runs without the pump, the fake TEI enforces the batch
    limit and can reject or hang, one poll budget.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(docker): install CA certificates in the runtime images
  ([`6ceda11`](https://github.com/thiesgerken/ferret/commit/6ceda11fce3fa1f6d396dd3aa391f902f7f2d28d))

  reqwest's rustls client panics at construction without system roots, so the server crashed at startup once it got an HTTP client for TEI.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(frontend): connection form states and labels
  ([`8afba01`](https://github.com/thiesgerken/ferret/commit/8afba01efde470e4d0c00000fad80d470da090fb))

  An unknown plugin shows an error instead of an empty form, create is disabled while pending, the card buttons name their connection, the delete confirmation takes focus, and required list fields are required.

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(server): sturdier runs and no connection over existing documents
  ([`9a5f4d4`](https://github.com/thiesgerken/ferret/commit/9a5f4d418699af8330a7abcb5d7763e113f961ff))

  - A connection can't be created where documents already live below its root: its first
    sync would sweep them. The error names how many.
  - The due-runs listener reconnects forever instead of dying with its first failed
    reconnect, and wakes claims after reconnecting.
  - finish is a tagged result: succeeded with state and stats, or failed with an error.
  - The internal client has connect and request timeouts, the public one connect and read
    timeouts.
  - The worker retries finish for about a minute and puts back log lines a failed heartbeat
    couldn't deliver.
  - ferret-api no longer depends on schemars.

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(server): match each word in any field, exclude before the candidate limit
  ([`4bf9c60`](https://github.com/thiesgerken/ferret/commit/4bf9c608533b61e5cf1df86edeb62516bf7fc6d5))

  Every query word now has to match in at least one of the German, English or trigram fields, so a compound and an inflected word can match one chunk. Excluded words drop documents before the 1000-chunk cut and also catch compounds. Adds the repeatable content_type= filter.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🐛 fix(chart): keep the database across uninstalls, require a hostname
  ([`c260676`](https://github.com/thiesgerken/ferret/commit/c26067632e1b2d877753c056af60a439745e6a0b))

  `helm uninstall` used to delete the index with its PVC. PVC and Secret now stay together, so a reinstall finds both. The HTTPRoute fails on a missing hostname instead of routing an example one, CI lints and renders the chart, and architecture.md names /auth and the HTTPRoute.

  Part of #19

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(chart): probe readiness on /healthz
  ([`3a75cb6`](https://github.com/thiesgerken/ferret/commit/3a75cb6ef2584966ebb2f18adaaa8a78edf22630))

  The health check moved out of /api, which only takes bearer credentials. The HTTPRoute doesn't expose it.

  Part of #19

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🐛 fix(compose): cap TEI's batch tokens so its warmup isn't OOM-killed
  ([`246d14c`](https://github.com/thiesgerken/ferret/commit/246d14c202bc29d1d81fd5eb37f4180f7cad6058))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### 💄 UI/UX


- 💄 style(ui): a home screen with the big mark, an example query and the index size
  ([`38bd11f`](https://github.com/thiesgerken/ferret/commit/38bd11fafaf543d3e57a16a7027b153254c8c5a2))

- 💄 style(ui): a home screen with the big mark, an example query and the index size
  ([`7e15d64`](https://github.com/thiesgerken/ferret/commit/7e15d6494c03cda60b9bcf9920b74759b495cc72))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 💄 style(ui): a filter column on wide screens, skeletons and an empty state
  ([`6d68000`](https://github.com/thiesgerken/ferret/commit/6d68000efa324055f2f28b8dc518857d94c5dd82))

- 💄 style(ui): a filter column on wide screens, skeletons and an empty state
  ([`af1342b`](https://github.com/thiesgerken/ferret/commit/af1342b94dcf155ef2e403383977af48b2582d0a))

  On wide screens the filters move into a column next to the hits, with labelled lists for time, sources and authors, and the header lines the search field up with the hits. On narrow screens they stay one horizontally scrolling chip row instead of wrapping. Hit-shaped placeholders stand in until the first answer, and no matches gets an empty state that offers to clear the filters.

  Part of #84

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 💄 style(ui): relevance bars, plugin icons and a calmer hit layout
  ([`bec1009`](https://github.com/thiesgerken/ferret/commit/bec1009e66d506e517a30fcd15df1d15c4b59834))

- 💄 style(ui): relevance bars, plugin icons and a calmer hit layout
  ([`e9f36ac`](https://github.com/thiesgerken/ferret/commit/e9f36acf5037601e47df7abde967e2e94525b693))

  Four signal bars replace "53% similar · keyword match": the document's best similarity between the model's cut-off and 0.75, in the brand color when every word matched and grey when only the meaning did, with the exact values in a tooltip. The source line gets the owning plugin's icon and leaves out a missing author instead of "· ·". The ✓ and the origin link show on hover, and highlights are bold instead of boxed.

  Part of #84

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 💄 feat(ui): Markdown hits show plain text in their snippets
  ([`81ca00d`](https://github.com/thiesgerken/ferret/commit/81ca00dad00cb04f71a907c6433c70e57ed72e69))

- 💄 feat(ui): snippets also handle Obsidian lists, tasks and wiki links
  ([`8c07705`](https://github.com/thiesgerken/ferret/commit/8c07705df518c72c54b342b0bb6f150150793eb3))

  - `__` emphasis only outside words and not around a lone word, so `__init__` and `foo__bar__baz`
    survive.
  - Ordered list markers, task checkboxes and bullets at any depth go.
  - Wiki links and embeds keep their alias, or else the note's name.
  - The doc comment says tables, HTML and fences stay raw on purpose.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 💄 feat(ui): Markdown hits show plain text in their snippets
  ([`d39ef43`](https://github.com/thiesgerken/ferret/commit/d39ef43149e28d66d9fb6c7e9520445039c822af))

  Heading markers, blockquotes, list bullets, emphasis, inline code and link targets are removed from Markdown snippets. Stripping works on the joined chunk text and splits it back, so the highlights stay on the words they marked.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 🔧 Configuration


- 🔧 chore(search): a calibrated similarity cut-off for Qwen3, 0.42
  ([`329ae83`](https://github.com/thiesgerken/ferret/commit/329ae838aafdf6a9cf9801b571a7fef0b2aca5ce))

- 🔧 chore(search): a calibrated similarity cut-off for Qwen3, 0.42
  ([`c23516f`](https://github.com/thiesgerken/ferret/commit/c23516f304cb1a6d5b954369bda8e2428fa30633))

  Measured on the compose demo corpus. Part of #72.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🔧 chore(dev): a second TEI for the embed pump, so queries don't wait for backfills
  ([`135557a`](https://github.com/thiesgerken/ferret/commit/135557acea43f5fe9320b16fdeb135311443f2b7))

- 🔧 chore(dev): a second TEI for the embed pump, so queries don't wait for backfills
  ([`8b917e6`](https://github.com/thiesgerken/ferret/commit/8b917e67522b3edc2c6f538d8e3dc49b9e12599b))

  With one TEI, a query embedding queued behind the pump's CPU batches (about 30 s each) and ran into the 5 s query timeout, so every search during a backfill came back keyword-only. tei-bulk is the model's bulk_url and serves only the pump. The query TEI has four times its CPU weight.

  Closes #77

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🔧 chore(dev): demo seed for the compose setup
  ([`39e59ff`](https://github.com/thiesgerken/ferret/commit/39e59ffdd3e7a23fad59e9401ed91fee1dc3326c))

- 🔧 chore(dev): the demo seed waits for the server and the worker
  ([`93d5e10`](https://github.com/thiesgerken/ferret/commit/93d5e10d4611e842a272d644ec5449dfd57e6e93))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🔧 chore(dev): demo seed for the compose setup
  ([`4b83a63`](https://github.com/thiesgerken/ferret/commit/4b83a6336a0d9fc8e42db0f97898116145759207))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🔧 chore(chart): tolerate the GPU taint by default, request GPU TEI memory
  ([`111ab87`](https://github.com/thiesgerken/ferret/commit/111ab87a733812a0ac46f00da9bbdd57c689157d))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🔧 chore: embedding config and TEI per model in compose and the chart
  ([`a910cfd`](https://github.com/thiesgerken/ferret/commit/a910cfd4400053f3af2edc883dc7d5c3405bad09))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🔧 chore: move compose host ports to 8380 and 5439
  ([`b9c7335`](https://github.com/thiesgerken/ferret/commit/b9c7335f6ec3c150f4ecc9a6af1a8d221eed5d88))

  8080 and 5432 are commonly held by other local stacks, which made `docker compose up` fail to bind.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🔧 chore(ui): proxy to the docker-compose server by default
  ([`0a681cd`](https://github.com/thiesgerken/ferret/commit/0a681cd6d4096f8f934de086e42356a6d0bfdc15))

  docker-compose publishes the server on 127.0.0.1:8380, since 8080 is often taken.

  Part of #1

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 🩹 Patches


- 🩹 fix(plugins): an empty or non-Unicode FERRET_GIT_PROTOCOLS fails at startup
  ([`3bcd4d7`](https://github.com/thiesgerken/ferret/commit/3bcd4d795f94bba792e8197e2ae910759e968dd9))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🩹 fix(readme): a static license badge, the GitHub one renders "invalid"
  ([`b8ddfe2`](https://github.com/thiesgerken/ferret/commit/b8ddfe2064db6b7cebcb4a7eca75a7c88d513a07))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### ♻️ Refactoring


- ♻️ refactor(ui): one HomeLink for the headers' mark and name
  ([`2042e50`](https://github.com/thiesgerken/ferret/commit/2042e50162a590670ef2d22eb38413029985c369))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ♻️ refactor(plugins): Paperless matches on PutResponse
  ([`833ea3c`](https://github.com/thiesgerken/ferret/commit/833ea3c6d6f78d879069c31b8cbd3bef252a62f5))

  The Paperless plugin landed on integration/v1 with the old PutOutcome. An excluded document counts as neither put nor unchanged, like in git and carapace. Also regenerates the schema after the merge.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ♻️ refactor(frontend): Explore links to the viewer through viewerHref
  ([`55da88c`](https://github.com/thiesgerken/ferret/commit/55da88c4ccbcfcca81363399963ecc54e80aa313))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ♻️ refactor(plugins): move repository handling into a shared repo module
  ([`42fad7e`](https://github.com/thiesgerken/ferret/commit/42fad7ecceabf05e1bc9d5748a34e6413fee463c))

  carapace (#13) clones, lists, reads and diffs repositories the same way.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ♻️ refactor(server): search and document reads as service functions for both APIs
  ([`8168020`](https://github.com/thiesgerken/ferret/commit/8168020b26a8d92c096c471d4c4bf1e63703e723))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- ♻️ refactor(server): authenticate /api callers in api_auth, refuse admin keys with sources
  ([`31cb17f`](https://github.com/thiesgerken/ferret/commit/31cb17f7cadc277ed1c8d61de904c6cb8a3f7efe))

  ApiCaller and Scope move to api_auth.rs, which dispatches bearer tokens by kind, so plugin run tokens can produce the same caller later. ApiCaller exposes its source roots for filtering in SQL. Admin scopes act on users, which have no source, so creation refuses them together with sources instead of silently ignoring the restriction. New tests cover the boundary between users' keys and the UI's error path on a 400.

  Part of #3

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- ♻️ refactor(server): split migrate from router, health check at /healthz
  ([`cdebbcd`](https://github.com/thiesgerken/ferret/commit/cdebbcddbd63db406ecb90d66b7fcdbd21cad559))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### ⚡ Performance


- ⚡️ perf(plugins): carapace indexes only what changed since the last run
  ([`3bb7ca1`](https://github.com/thiesgerken/ferret/commit/3bb7ca16eb92eb2b58995630607cfbfefd364333))

- ⚡️ perf(plugins): carapace indexes only what changed since the last run
  ([`36b1a51`](https://github.com/thiesgerken/ferret/commit/36b1a519057cbe7e79995bc0fcdbb59dc3938ba7))

  A run diffs against the last indexed commit and puts the changed files' documents. What a changed or deleted file held at that commit (`git show <cursor>:<path>`) but no longer does is deleted, so facts a re-rendered session dropped go away without a sweep. A full sync remains for the first run, a force push, a new carapace_url, and an old version that doesn't parse.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

### 🔒 Security


- 🔒️ fix(admin): disabling a user stops their runs in flight, health checks run at once
  ([`3e80464`](https://github.com/thiesgerken/ferret/commit/3e804646098ffa516c1c836f18d66fd0aed6c09e))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🔒️ fix(server): credentials survive only an unchanged config, and stay bound to their row
  ([`62b1efa`](https://github.com/thiesgerken/ferret/commit/62b1efa04087c8ea294cfc30f422f5a95a53c223))

  - Stored credentials are kept only by an update that leaves the rest of the config as it is.
    Any other change (remote, branch, globs) needs every stored credential entered again.
    Otherwise a session could point the remote at its own server, and the next run would send
    the token there.
  - The sealed credentials are bound to their user and connection as associated data, so a blob
    copied to another row doesn't open. Connection ids now come from Rust to make that
    possible.
  - Validation errors under a secret field, or ones that would quote a secret, only name the
    path.
  - Credentials that can't be opened anymore need all secret fields entered again, not just
    one.
  - `null` removes a credential, tested.
  - ClaimedRun's Debug leaves out config and token.
  - The docs describe the column, the rules and the git token's basic auth.

  Part of #11

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🔒️ fix(frontend): never link origin urls with dangerous schemes
  ([`07197c7`](https://github.com/thiesgerken/ferret/commit/07197c76ce5010663573e26c25adfbc838fbab89))

- 🔒️ fix(frontend): never link origin urls with dangerous schemes
  ([`654bba7`](https://github.com/thiesgerken/ferret/commit/654bba717902a648f91803baf7e18a6dca242f4c))

  The hit list and the viewer only offer "Open in origin" for urls that linkableUrl accepts, so a stored javascript:, data: or vbscript: url renders as no link at all instead of relying on React's own guard.

  Part of #33

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🔒️ fix(plugins): the git plugin reads only regular files and clones only over HTTPS
  ([`4e5ab5b`](https://github.com/thiesgerken/ferret/commit/4e5ab5badb9161f419b0b01fef8ba5c0d26d31cc))

  A repository could carry a symlink to the worker's own files (the pod's service account token, other runs' checkouts) and get them indexed for whoever created the connection. The plugin now indexes only regular files by their git mode, double-checks the checkout without following links, and skips symlinks and submodules with a log line. The plugins pods no longer mount a service account token at all.

  Also:
  - GIT_ALLOW_PROTOCOL limits remotes to HTTPS (a worker option the tests widen to file).
  - git is killed when the run is dropped and fails after 30 minutes.
  - Files above 1 MiB are skipped before they're read.
  - git log -z: paths git would C-quote no longer go missing, and a tracked file without
    history fails the run instead of vanishing.
  - url_template percent-encodes each path segment.
  - The log parser has its own fixture test.

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 🔒 fix: reject javascript:, data: and vbscript: URLs
  ([`bff3463`](https://github.com/thiesgerken/ferret/commit/bff34637b34a2911047abad8f97cbde27b6c1fc5))

- 🔒 fix: reject javascript:, data: and vbscript: URLs at ingest
  ([`ee8db99`](https://github.com/thiesgerken/ferret/commit/ee8db99e0186e49bb9eeb43fb8074d5a629cc1f5))

  The server answers such a document url with 400, and the frontend gets the same check (linkableUrl) for rendering links.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🔒 fix(server): keep the Argon2 permit until the hash is done, cap at 4
  ([`ff53e68`](https://github.com/thiesgerken/ferret/commit/ff53e68ffa0c8958c8d0eba1a7deba95ab1345be))

  Dropping a login's future freed the permit while the blocking hash kept running, so aborted requests got around the bound.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 🔒 fix(server): run at most one Argon2 hash per core at a time
  ([`bf1f853`](https://github.com/thiesgerken/ferret/commit/bf1f853cf3f29cb6e85e282152b17686b7f22d67))

  Logins are public and each hash allocates ~19 MiB, so a burst of them could OOM-kill the server pod.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

### 👷 CI/Build


- 👷 ci: pass FERRET_CONFIG_KEY in compose and the chart
  ([`6493efa`](https://github.com/thiesgerken/ferret/commit/6493efa5f5ce8c3832a6539bc80d147678e23ce0))

  The chart generates a config-key once (randBytes 32) and keeps it across upgrades like the other secrets. The docs describe the key's format and how to recover from losing it.

  Part of #11

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 👷 ci: run plugin workers in compose and the chart
  ([`b54f0ca`](https://github.com/thiesgerken/ferret/commit/b54f0cac9e0f0de7e9217c4e6e98822ecf429a77))

  The server needs FERRET_WORKER_SECRET and serves the internal API on 8081. The chart generates a worker-secret key (kept on upgrades like the others), deploys the plugins image and adds a NetworkPolicy that admits only the plugin workers to the internal port. The docs describe the run lifecycle as built and tighten auth.md: API keys can't write below a connection's root.

  Part of #9

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 👷 ci: release from main and publish PR images
  ([`5979fa1`](https://github.com/thiesgerken/ferret/commit/5979fa17271e84c2aa7d097fa82d3a04a7bdadc7))

- 👷 ci: release from main and publish PR images
  ([`15354fa`](https://github.com/thiesgerken/ferret/commit/15354fa9bc5c690976d9116f8f0e771673cb15dc))

  semantic-release with carapace's emoji parser and tag lists versions the Cargo workspace, the frontend and the chart together. A release pushes the images ferret, ferret-plugins and ferret-frontend and the chart to ghcr. Every PR pushes :pr<N> images and a matching chart. The Dockerfile gains a plugins target next to the server.

  Part of #20

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F

- 👷 ci: check .sqlx against the migrated schema
  ([`b1d24d1`](https://github.com/thiesgerken/ferret/commit/b1d24d111312c1539ebef29125556c2165c34ebc))

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_01QBjAQrLb2qGNiYxmgQHByo

- 👷 ci(ui): add the frontend to docker-compose and CI
  ([`2f0fa62`](https://github.com/thiesgerken/ferret/commit/2f0fa622ffc1cf1b2cecebc0e39064b45ac764d1))

  docker-compose gains the frontend and a traefik proxy that serves UI and server on one origin (127.0.0.1:3380), routed like the cluster's HTTPRoute. CI lints, tests and builds the frontend image.

  Part of #1

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

  Claude-Session: https://claude.ai/code/session_0182NU5DS1c8iPNewjhSN63F
