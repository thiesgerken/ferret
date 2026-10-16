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

{{/* A model name as part of a Kubernetes name, which allows no dots or underscores. */}}
{{- define "ferret.modelSlug" -}}
{{- . | replace "." "-" | replace "_" "-" -}}
{{- end }}

{{/* The server's config.yaml (FERRET_CONFIG). */}}
{{- define "ferret.config" -}}
embedding:
  default: {{ .Values.embedding.default | quote }}
  models:
    {{- range .Values.embedding.models }}
    - name: {{ .name | quote }}
      url: "http://{{ $.Release.Name }}-tei-{{ include "ferret.modelSlug" .name }}:80"
      query_prefix: {{ .queryPrefix | quote }}
      document_prefix: {{ .documentPrefix | quote }}
    {{- end }}
{{- end }}
