#!/usr/bin/env bash
# Fills the compose dev setup with demo data: the documents in docker/demo, a git connection to
# ferret's own docs and, with CARAPACE_KNOWLEDGE_DIR set, a carapace connection to the repository
# docker-compose.demo.yml mounts. Re-running it updates the documents and skips existing
# connections.
set -euo pipefail

url=${FERRET_URL:-http://127.0.0.1:3380}
demo=$(cd "$(dirname "$0")/../docker/demo" && pwd)

# The session cookie is `Secure`, which curl's cookie jar won't send over plain http. The retries
# cover a server that is still starting (connection refused, or 502 from the proxy).
cookie=$(curl -fsS --retry 90 --retry-delay 2 --retry-connrefused -o /dev/null -D - "$url/auth/login" \
  -H 'content-type: application/json' \
  -d "$(jq -n --arg password "${FERRET_PASSWORD:-ferret-development}" \
    '{username: "admin", password: $password}')" |
  grep -i '^set-cookie:' | sed 's/^[^:]*: *//; s/;.*//')

graphql() { # query, variables
  curl -sS --fail-with-body "$url/graphql" -H "cookie: $cookie" -H 'content-type: application/json' \
    -d "$(jq -n --arg query "$1" --argjson variables "$2" '{$query, $variables}')" |
    jq -e 'if .errors then error(.errors | map(.message) | join("; ")) else .data end'
}

# The machine API takes only API keys, so the documents go in with one that works for this run
# only. Revoked, it stays their origin.
created=$(graphql 'mutation($input: NewApiKey!) { createApiKey(input: $input) { key apiKey { id } } }' \
  '{"input": {"name": "Demo data", "icon": "package", "scopes": ["INGEST_WRITE"], "sources": ["demo"]}}')
key=$(jq -r .createApiKey.key <<<"$created")
revoke_key() {
  graphql 'mutation($id: ID!) { revokeApiKey(id: $id) }' \
    "$(jq '{id: .createApiKey.apiKey.id}' <<<"$created")" >/dev/null
}
trap revoke_key EXIT

for document in "$demo"/*.json; do
  curl -sS --fail-with-body -X PUT "$url/api/documents" -H "authorization: Bearer $key" \
    -H 'content-type: application/json' --data @"$document" |
    jq -r --arg file "${document##*/}" '"\($file): \(.outcome)"'
done

existing=$(graphql '{ connections { source } }' '{}' | jq -r '.connections[].source')
connect() { # plugin, source, name, icon, config
  if grep -qx "$2" <<<"$existing"; then
    echo "connection $2: exists"
    return
  fi
  local input error
  input=$(jq -n --arg plugin "$1" --arg source "$2" --arg name "$3" --arg icon "$4" \
    --argjson config "$5" \
    '{input: {$plugin, $source, $name, $icon, config: ($config | tojson), schedule: "0 * * * *"}}')
  # Right after `docker compose up`, the worker may not have registered its plugins yet.
  for _ in {1..30}; do
    if error=$(graphql 'mutation($input: NewConnection!) { createConnection(input: $input) { id } }' \
      "$input" 2>&1 >/dev/null); then
      echo "connection $2: created, first run starts now"
      return
    fi
    [[ $error == *"no worker offers the plugin"* ]] || break
    sleep 2
  done
  echo "connection $2: $error" >&2
  return 1
}

connect git ferret "ferret docs" book-open '{
  "remote": "https://github.com/thiesgerken/ferret.git",
  "include": ["README.md", "docs/**"],
  "url_template": "https://github.com/thiesgerken/ferret/blob/{commit}/{path}"
}'
if [[ -n ${CARAPACE_KNOWLEDGE_DIR:-} ]]; then
  connect carapace carapace carapace brain '{"remote": "file:///demo/carapace", "carapace_url": "http://127.0.0.1:3001"}'
fi
