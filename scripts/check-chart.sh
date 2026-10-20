#!/usr/bin/env bash
# Lints and renders the chart with the values in charts/test-values, and checks that values it
# must refuse fail the render.
set -euo pipefail
cd "$(dirname "$0")/.."
chart=charts/ferret

for values in charts/test-values/*.yaml; do
  helm lint "$chart" -f "$values" >/dev/null
  helm template ferret "$chart" -f "$values" >/dev/null
done
# The defaults deploy no TEI. They name no endpoint either, so the model gets one here (`--set`
# on a list replaces the whole list, hence the model's other required fields).
defaults=$(helm template ferret "$chart" --set ingress.hostname=ferret.example.com \
  --set 'embedding.models[0].id=qwen3-embedding-0.6b' \
  --set 'embedding.models[0].dimension=1024' --set 'embedding.models[0].minSimilarity=0.42' \
  --set 'embedding.models[0].embeddingApi.url=http://tei/v1' \
  --set 'embedding.models[0].embeddingApi.model=Qwen/Qwen3-Embedding-0.6B')
if grep -q 'name: ferret-tei' <<<"$defaults"; then
  echo "the defaults render a TEI" >&2
  exit 1
fi

mounts=$(helm template ferret "$chart" -f charts/test-values/files-mounts.yaml --show-only templates/plugins.yaml)
for expected in 'replicas: 2' 'automountServiceAccountToken: false' 'server: nas.example.com' 'mountPath: "/mnt/documents"' 'readOnly: true'; do
  if [[ "$mounts" != *"$expected"* ]]; then
    echo "worker mounts missing: $expected" >&2
    exit 1
  fi
done
if [[ "$mounts" == *'privileged: true'* ]]; then
  echo "worker mounts require no privileged container" >&2
  exit 1
fi

# Renders that must fail, each with the message it must give.
refuse() {
  local expected=$1
  shift
  local output
  if output=$(helm template ferret "$chart" "$@" 2>&1); then
    echo "rendered, but expected: $expected ($*)" >&2
    exit 1
  fi
  if ! grep -q -- "$expected" <<<"$output"; then
    echo "expected \"$expected\", got: $output" >&2
    exit 1
  fi
}
bundled=(-f charts/test-values/bundled-tei.yaml)
endpoints=(-f charts/test-values/endpoints.yaml)
refuse "needs an embeddingApi or a queryApi" --set ingress.hostname=ferret.example.com
refuse "exactly one model, but 2" "${endpoints[@]}" --set bundledTei.enabled=true \
  --set 'embedding.models[0].bundledTei=true' --set 'embedding.models[0].embeddingApi=null' \
  --set 'embedding.models[0].queryApi=null' --set 'embedding.models[1].bundledTei=true' \
  --set 'embedding.models[1].embeddingApi=null'
refuse "exactly one model, but 0" "${endpoints[@]}" --set bundledTei.enabled=true
refuse "bundledTei.enabled is off" "${bundled[@]}" --set bundledTei.enabled=false
refuse "sets bundledTei and endpoints" "${bundled[@]}" --set 'embedding.models[0].queryApi.url=http://tei/v1'
refuse "name is now id" "${endpoints[@]}" --set 'embedding.models[1].name=bge-m3'
refuse "belong to a TEI" "${endpoints[@]}" --set 'embedding.models[1].modelId=BAAI/bge-m3'
refuse "were replaced by per-model endpoints" "${endpoints[@]}" --set 'externalTeis[0]=http://laptop:8080'
refuse "were replaced by per-model endpoints" "${endpoints[@]}" --set tei.internal.enabled=true
refuse "tokenSecret needs a name and a key" "${endpoints[@]}" --set 'embedding.models[1].embeddingApi.tokenSecret.name=x'
echo "chart checks passed"
