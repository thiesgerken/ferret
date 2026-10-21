{{- define "ferret.podSecurityContext" -}}
runAsNonRoot: true
runAsUser: 65532
runAsGroup: 65532
fsGroup: 65532
seccompProfile:
  type: RuntimeDefault
{{- end }}

{{- define "ferret.containerSecurityContext" -}}
readOnlyRootFilesystem: true
allowPrivilegeEscalation: false
capabilities:
  drop: [ALL]
{{- end }}

{{- define "ferret.podRuntime" -}}
automountServiceAccountToken: false
securityContext:
  {{- include "ferret.podSecurityContext" . | nindent 2 }}
nodeSelector:
  kubernetes.io/arch: amd64
{{- end }}

{{- define "ferret.labels" -}}
app.kubernetes.io/name: ferret
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end }}

{{/* Selector labels of one component; pass (dict "root" $ "component" "server"). */}}
{{- define "ferret.selectorLabels" -}}
app.kubernetes.io/name: ferret
app.kubernetes.io/instance: {{ .root.Release.Name }}
app.kubernetes.io/component: {{ .component }}
{{- end }}

{{- define "ferret.image" -}}
{{ .image.registry }}/{{ .image.repository }}:{{ .image.tag | default .root.Chart.AppVersion }}
{{- end }}

{{- define "ferret.secretName" -}}
{{- .Values.secrets.existingSecret | default (printf "%s-secrets" .Release.Name) -}}
{{- end }}

{{- define "ferret.databaseUrl" -}}
{{- if hasKey .Values.postgres "external" -}}
{{- .Values.postgres.external.url -}}
{{- else -}}
{{- printf "postgres://%s@%s-postgres:5432/%s" .Values.postgres.username .Release.Name .Values.postgres.database -}}
{{- end -}}
{{- end }}

{{- define "ferret.databasePasswordSecret" -}}
{{- if hasKey .Values.postgres "external" }}
name: {{ .Values.postgres.external.passwordSecret.name | quote }}
key: {{ .Values.postgres.external.passwordSecret.key | quote }}
{{- else }}
name: {{ include "ferret.secretName" . }}
key: postgres-password
{{- end }}
{{- end }}

{{/* The environment variable holding an endpoint's token; pass (dict "model" . "role" "EMBEDDING").
Environment variable names allow no dots or dashes. */}}
{{- define "ferret.tokenEnv" -}}
FERRET_TOKEN_{{ .model.id | upper | replace "." "_" | replace "-" "_" }}_{{ .role }}
{{- end }}

{{/* An endpoint of the server's config; pass (dict "api" .embeddingApi "model" . "role" "EMBEDDING"). */}}
{{- define "ferret.api" -}}
url: {{ required (printf "model %q: an endpoint needs a url" .model.id) .api.url | quote }}
model: {{ required (printf "model %q: an endpoint needs a model" .model.id) .api.model | quote }}
{{- with .api.tokenSecret }}
{{- if not (and .name .key) }}
{{- fail (printf "model %q: tokenSecret needs a name and a key" $.model.id) }}
{{- end }}
token_env: {{ include "ferret.tokenEnv" (dict "model" $.model "role" $.role) }}
{{- end }}
{{- with .api.concurrency }}
concurrency: {{ . }}
{{- end }}
{{- end }}

{{/* Fails on obsolete values, invalid external Postgres settings, and a bundled TEI without
exactly one model for it. */}}
{{- define "ferret.checkValues" -}}
{{- if hasKey .Values.postgres "external" }}
{{- $external := .Values.postgres.external }}
{{- if not $external.url }}
{{- fail "postgres.external needs a url" }}
{{- end }}
{{- if or (regexMatch "^[^:]+://[^/?#@]*:[^/?#@]*@" $external.url) (regexMatch "(?i)[?&]password=" $external.url) }}
{{- fail "postgres.external.url must not contain a password; use passwordSecret" }}
{{- end }}
{{- $password := $external.passwordSecret | default dict }}
{{- if not (and $password.name $password.key) }}
{{- fail "postgres.external.passwordSecret needs a name and a key" }}
{{- end }}
{{- if hasKey $external "caSecret" }}
{{- $ca := $external.caSecret | default dict }}
{{- if not (and $ca.name $ca.key) }}
{{- fail "postgres.external.caSecret needs a name and a key" }}
{{- end }}
{{- end }}
{{- end }}
{{- if or .Values.tei .Values.externalTeis }}
{{- fail "tei and externalTeis were replaced by per-model endpoints (embeddingApi, queryApi) and the optional bundledTei, see the chart README" }}
{{- end }}
{{- $bundled := list }}
{{- range .Values.embedding.models }}
{{- if .name }}
{{- fail (printf "model %q: name is now id (keep the value to keep its embeddings)" .name) }}
{{- end }}
{{- $id := required "embedding.models[].id is required" .id }}
{{- if or .modelId .maxBatchTokens .cacheSize .resources .bulkUrl }}
{{- fail (printf "model %q: modelId, maxBatchTokens, cacheSize, resources and bulkUrl belong to a TEI: set bundledTei, or name your own endpoint in embeddingApi, see the chart README" $id) }}
{{- end }}
{{- if .bundledTei }}
{{- if not $.Values.bundledTei.enabled }}
{{- fail (printf "model %q sets bundledTei, but bundledTei.enabled is off" $id) }}
{{- end }}
{{- if or .embeddingApi .queryApi }}
{{- fail (printf "model %q sets bundledTei and endpoints: the bundled TEI is its endpoint" $id) }}
{{- end }}
{{- $bundled = append $bundled $id }}
{{- else if not (or .embeddingApi .queryApi) }}
{{- fail (printf "model %q needs an embeddingApi or a queryApi, or bundledTei: true" $id) }}
{{- end }}
{{- end }}
{{- if and .Values.bundledTei.enabled (ne (len $bundled) 1) }}
{{- fail (printf "bundledTei serves exactly one model, but %d set bundledTei: true" (len $bundled)) }}
{{- end }}
{{- end }}

{{/* The server's config.yaml (FERRET_CONFIG_FILE). */}}
{{- define "ferret.config" -}}
{{- include "ferret.checkValues" . -}}
embedding:
  default: {{ .Values.embedding.default | quote }}
  models:
    {{- range $model := .Values.embedding.models }}
    - id: {{ .id | quote }}
      dimension: {{ required "embedding.models[].dimension is required" .dimension }}
      {{- with .queryPrefix }}
      query_prefix: {{ . | quote }}
      {{- end }}
      {{- with .documentPrefix }}
      document_prefix: {{ . | quote }}
      {{- end }}
      min_similarity: {{ required "embedding.models[].minSimilarity is required" .minSimilarity }}
      {{- if .bundledTei }}
      embedding_api:
        url: "http://{{ $.Release.Name }}-tei:80/v1"
        model: {{ $.Values.bundledTei.modelId | quote }}
      {{- end }}
      {{- with .embeddingApi }}
      embedding_api:
        {{- include "ferret.api" (dict "api" . "model" $model "role" "EMBEDDING") | nindent 8 }}
      {{- end }}
      {{- with .queryApi }}
      query_api:
        {{- include "ferret.api" (dict "api" . "model" $model "role" "QUERY") | nindent 8 }}
      {{- end }}
    {{- end }}
{{- end }}

{{/* The server's environment variables for the endpoints' tokens. */}}
{{- define "ferret.tokenEnvs" -}}
{{- range $model := .Values.embedding.models }}
{{- range $role, $api := dict "EMBEDDING" $model.embeddingApi "QUERY" $model.queryApi }}
{{- with $api }}
{{- with .tokenSecret }}
- name: {{ include "ferret.tokenEnv" (dict "model" $model "role" $role) }}
  valueFrom:
    secretKeyRef:
      name: {{ .name }}
      key: {{ .key }}
{{- end }}
{{- end }}
{{- end }}
{{- end }}
{{- end }}
