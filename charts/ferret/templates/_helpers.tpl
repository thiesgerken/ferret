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

{{/* The name of a model's internal TEI Deployment and Service; pass (dict "root" $ "model" .).
Kubernetes names allow no dots or underscores. */}}
{{- define "ferret.teiName" -}}
{{- $name := printf "%s-tei-%s" .root.Release.Name (.model.name | replace "." "-" | replace "_" "-") -}}
{{- if not (regexMatch "^[a-z]([-a-z0-9]{0,61}[a-z0-9])?$" $name) -}}
{{- fail (printf "model %q: %q is no valid Kubernetes name (DNS-1035 label of at most 63 characters)" .model.name $name) -}}
{{- end -}}
{{- $name -}}
{{- end }}

{{/* The server's config.yaml (FERRET_CONFIG_FILE). */}}
{{- define "ferret.config" -}}
{{- /* Values of earlier chart versions, which would otherwise be ignored without a word. */}}
{{- if .Values.tei.gpu }}
{{- fail "tei.gpu was replaced by externalTeis: deploy the GPU TEI separately and list its URL there, see the chart README" }}
{{- end }}
{{- range .Values.embedding.models }}
{{- if .bulkUrl }}
{{- fail (printf "model %q: bulkUrl was replaced by externalTeis, see the chart README" .name) }}
{{- end }}
{{- end }}
embedding:
  default: {{ .Values.embedding.default | quote }}
  models:
    {{- range .Values.embedding.models }}
    - name: {{ .name | quote }}
      model_id: {{ required "embedding.models[].modelId is required" .modelId | quote }}
      dimension: {{ required "embedding.models[].dimension is required" .dimension }}
      {{- with .queryPrefix }}
      query_prefix: {{ . | quote }}
      {{- end }}
      {{- with .documentPrefix }}
      document_prefix: {{ . | quote }}
      {{- end }}
      min_similarity: {{ required "embedding.models[].minSimilarity is required" .minSimilarity }}
    {{- end }}
  {{- if not (or .Values.tei.internal.enabled .Values.externalTeis) }}
  {{- fail "no TEI: enable tei.internal or list externalTeis" }}
  {{- end }}
  teis:
    {{- if .Values.tei.internal.enabled }}
    {{- range .Values.embedding.models }}
    - url: "http://{{ include "ferret.teiName" (dict "root" $ "model" .) }}:80"
    {{- end }}
    {{- end }}
    {{- range .Values.externalTeis }}
    - url: {{ . | quote }}
      external: true
    {{- end }}
{{- end }}
