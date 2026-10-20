# Mounted files

The `files` plugin discovers a Linux-mounted filesystem tree. Each included directory, including the share root and empty directories, becomes a document listing exactly its immediate children. Selected UTF-8 files also become complete-content documents. PDFs, Office documents, images, music and videos remain discoverable by filename and metadata in directory documents. No converters run, no original binary files are stored, and Ferret does not serve or proxy files.

## Mount the origin first

Kubernetes supplies read-only volumes to **every plugin worker replica**, not to the server, frontend or database. Workers need neither privileged containers nor Kubernetes API credentials. Example Helm values for a generic NFS export:

```yaml
plugins:
  volumes:
    - name: documents
      nfs:
        server: nas.example.com
        path: /exports/documents
        readOnly: true
  volumeMounts:
    - name: documents
      mountPath: /mnt/documents
```

An existing NFS-backed PVC can instead be supplied as a `persistentVolumeClaim` volume. k3s nodes need the appropriate NFS client support and network access to the export. The chart forces these worker mounts read-only and does not use `subPath`. Apply the values in the infrastructure repository that owns the deployment, then deploy the updated worker and server together. The server automatically applies the inventory migration at startup. No NAS daemon, watcher, snapshot service or application-side mount command is required.

The configured `root` must be an **actual mount point** present in Linux `/proc/self/mountinfo`, not a subdirectory inside a larger mount, the container root, a symlink or an unmounted placeholder directory. Mount the intended directory directly at that path. The plugin checks availability before scanning and again before sweeping, and verifies that the root identity did not change. A mounted empty share is valid and retains its root document. A missing mount fails without a sweep. An NFS server that presents an available but unexpectedly empty export cannot be distinguished from a genuinely empty share by this client-side check; export identity and mount correctness remain deployment responsibilities. Non-Linux workers fail safely because mount verification is Linux-specific.

All workers advertising `files` must have the same roots and access. A replacement worker without a root fails rather than deleting indexed content. Never expose worker mounts containing data a connection owner should not index: connection ownership controls access to indexed contents and filenames; NAS ACLs are not mirrored.

## Create a connection

Once the worker registers `files`, select it in **Connections → Create**. Enter a name, icon (for example `folder`), source root (for example `files/documents`), schedule and plugin fields:

```json
{
  "root": "/mnt/documents",
  "origin_url_prefix": "nfs://nas.example.com/exports/documents/",
  "content_globs": ["**/*.custom", "**/unusual-name"],
  "exclude_globs": ["**/private", "**/target"],
  "max_file_size": 1048576
}
```

The schema-driven form describes additive filters. `root` and `origin_url_prefix` are required. Both glob lists default to empty, extending rather than replacing bundled behavior. `max_file_size` defaults to 1 MiB (1,048,576 bytes), with an inclusive byte boundary. A zero limit permits only empty text files. The limit applies to individual plaintext files, never generated directory listings.

Globs use the existing Git plugin's `globset` syntax against slash-separated share-relative paths. Match a directory itself (for example `**/private`) to prune its entire subtree. `**/private/**` alone matches descendants, not the parent name. Ordinary hidden entries remain eligible. `Archive`, `Backup`, `build`, `dist`, `target` and `vendor` are not bundled exclusions.

Use existing UTC cron scheduling, for example `0 2,14 * * *` twice daily, or **Run now**. Every run performs a complete metadata walk; there is no new scheduler or filesystem watcher.

## Documents and origin links

All documents belong directly to the connection's configured source root, with no filesystem-derived sources. External identities are `dir:` for the root, `dir:Projects/demo` for a directory and `file:Projects/demo/main.rs` for file contents. A rename removes the old identity and creates the new one on successful reconciliation. File and directory namespaces cannot collide when a path changes kind.

Directory text contains its share-relative path (`/ (share root)` at the root) and alphabetically sorted JSON lines with immediate-child names, kinds and nanosecond UTC modification timestamps. Regular files also have sizes in bytes. JSON escaping keeps newline, quotation-mark and Markdown-like names unambiguous. Symlinks describe the link's metadata, not its target, and are never followed, including loops and links outside the mount. Special filesystem objects are listing-only. Non-UTF-8 filenames and unsupported timestamps fail explicitly rather than silently omitting names. Directory listings are never truncated; an ingestion/request hard limit fails the run without a sweep.

File documents contain complete text or nothing. `.md` and `.markdown` use Markdown content type; all other selected files are plain text. Origin creation dates and authors remain unknown. File modification dates come from filesystem mtime, not ingestion time or ctime. Directory modification dates are unknown because the directory's own mtime does not cover edits to existing children. Metadata includes `kind` and the share-relative `path`.

Origin links append separately encoded path segments to the independently configured URL prefix. The prefix must be hierarchical, end in `/`, and contain no query or fragment. NFS links require a host. Spaces, Unicode, `#`, `?` and `%` in filenames are encoded. Directories have trailing slashes, including the root. Configure the actual client-visible NFS namespace; the worker path is never inferred as a browser URL. Ferret's safe-link policy permits `nfs:`, but opening it requires a client-side protocol handler and is not universally supported.

## Bundled content selection

Additional content globs extend these case-sensitive extensions:

```text
tex md markdown txt rst org adoc py pyi ts tsx js jsx mjs cjs rs json jsonc
 yaml yml toml xml html htm css scss sass less sh bash zsh fish c h cc cpp hpp
cs java kt kts go rb php pl pm lua r R jl swift m mm ex exs erl hrl hs lhs
clj cljs scala sql graphql gql proto vue svelte ini cfg conf properties env
nix tf tfvars cmake make mk dockerfile csv tsv log bib sty cls
```

Conventional extensionless/dotfile names are also selected, at any depth:

```text
README LICENSE LICENCE COPYING NOTICE AUTHORS CHANGELOG CONTRIBUTING Makefile
GNUmakefile Dockerfile Containerfile Justfile justfile Gemfile Rakefile Procfile
.gitignore .gitattributes .editorconfig .env
```

Valid UTF-8 includes ASCII. Selected invalid-encoding, NUL-containing/binary or oversized files are intentional logged skips, remaining visible in their parent listing. The plugin never stores a truncated prefix, executes files, unpacks archives or converts encodings. Unexpected selected-file read errors fail the run. Changes observed between enumeration and reading also fail safely and can be retried against the live tree.

## Mandatory exclusions

Additional exclusions never disable these bundled rules at any depth:

- Directories: `.git`, `.hg`, `.svn`, `node_modules`, `.venv`, `venv`, `__pycache__`, `.mypy_cache`, `.pytest_cache`, `.ruff_cache`, `.next`, `.nuxt`, `.zfs`, `.snapshot`, `.stversions`, `.Trash`, `.Trashes`, `$RECYCLE.BIN`.
- Desktop metadata names: `.DS_Store`, `Thumbs.db`, `desktop.ini`.

Excluded subtrees are pruned before traversal and excluded names do not appear in listings. Filesystem filters are distinct from the existing remembered document/thread exclusions in Connections. Remembered document exclusions do not recursively exclude filesystem directories.

## Inventory, recovery and diagnostics

The server stores connection-scoped metadata inventory in Postgres, accessed through live-run-token HTTP requests in batches of at most 512. The cursor contains only a generation UUID and indexing-settings fingerprint, never a per-file JSON map. Workers do not persist inventory locally or access the database. Fingerprints include kind, size, high-resolution mtime/ctime and device/inode metadata. Every run still enumerates and stats the included tree, examining children even when their parent mtime is unchanged. Unchanged eligible bodies are not reread or uploaded, and their document identities are marked seen. Directory documents are rebuilt deterministically; unchanged puts do not change content or trigger new embeddings.

A generation is staged only after its associated document operations succeed. Only successful finish publishes it for reuse; superseded generations are removed, and abandoned generations are reclaimed on the next claim. Failed/lost runs force a full reconciliation from empty working state. Removing a remembered exclusion also resets that state. Changing selection, exclusions, size limit, origin prefix, mount root or rendering version invalidates reuse and reconciles unchanged entries too.

A sweep happens only after complete enumeration, required writes, staged inventory, and final mount checks succeed. Unexpected enumeration, permission, I/O, selected-read or ingestion errors fail without a sweep. Earlier successful puts may remain because a run is not a transaction over the origin. A retry reconciles those partial writes. Successful scans remove deleted, renamed, excluded and newly ineligible documents. This is a live-tree walk, not an atomic NAS snapshot.

Activity logs report intentional eligibility skips, failed operations and a final directory/file/body-read count. A no-change repeat should report zero file bodies read. Investigate mount availability, permissions and request-size limits before retrying an incomplete scan. Logs contain paths and reasons, never file contents. Use keyword search to verify indexing independently of embedding completion.
