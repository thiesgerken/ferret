# Git repositories

Use `git` for text files in a repository, including an Obsidian vault. The worker needs Git, network access to the remote, and read-only repository access. Public HTTPS repositories need no token. For a private repository use a fine-grained token restricted to reading that repository's contents; the plugin sends it as HTTP Basic authentication with username `x-access-token`.

For automatic discovery of a Gitea account's repositories and open issue/PR discussions, use [Gitea](gitea.md).

## Set up

In **Connections**, create plugin `git`, source `notes/git`, choose a name/icon, and schedule `0 * * * *`. Enter:

```json
{
  "remote": "https://github.com/owner/notes.git",
  "branch": "main",
  "include": ["**/*.md", "README.md"],
  "exclude": ["private/**", ".obsidian/**"],
  "markdown": ["**/*.md"],
  "url_template": "https://github.com/owner/notes/blob/{commit}/{path}"
}
```

Replace owner/repository and branch. Add `token` only for a private HTTPS remote. Omit `branch` to follow the remote's default branch. Empty/omitted `include` selects everything; `exclude` wins. `markdown` defaults to `**/*.md`; other selected text is plain. All fields except `remote` are optional. Unknown fields are rejected.

`url_template` substitutes the indexed commit and URL-encoded path segments. For GitLab use its `/-/blob/{commit}/{path}` URL; a vault URL must be meaningful on the user's device. Without a template, use Ferret's viewer. Ferret does not infer a forge from the remote.

## Mapping and synchronization

Each regular UTF-8 text file of at most 1 MiB becomes one document at the connection source. `external_id` is its repository-relative path; no threads are created. The first Markdown heading becomes the title, otherwise a readable file-name title is used for Markdown. Plain files keep their file name. Git history provides the first/last author dates and latest author name.

The worker clones full history of one branch into a temporary directory for each run. The first run performs a full sync and sweep; later runs apply the diff from the saved commit, explicitly deleting removed paths. Renames are deletion plus addition. A missing ancestor commit or changed indexing settings causes a full sync. A file becoming binary, oversized, a symlink or a submodule is not indexed and its old document is removed. A failed run does not save the new cursor.

Open **Activity** after creation. A successful first run logs the cloned commit and full-sync reason, followed by put/unchanged/deleted counts. Search a known title in keyword mode with the connection source filter, then verify the origin link opens the indexed file.

## Limits and failures

Only committed files on the selected branch are visible. Binary/NUL-containing files, symlinks, submodules and files over 1 MiB are skipped. Remote Git commands time out after 30 minutes. The default worker transport allowlist is HTTPS; SSH, HTTP and `file:` remotes require an explicit operator change to `FERRET_GIT_PROTOCOLS`. Do not enable local-file transport for untrusted connection editors: it broadens worker filesystem access. Tokens do not provide SSH authentication.

See [connection lifecycle](../connections.md). Source: [git.rs](../../crates/ferret-plugins/src/plugins/git.rs), [repository transport](../../crates/ferret-plugins/src/plugins/repo.rs).
