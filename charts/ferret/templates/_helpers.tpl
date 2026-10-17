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

{{/* The name of a model's TEI Deployment and Service; pass (dict "root" $ "model" .). Kubernetes
names allow no dots or underscores. */}}
{{- define "ferret.teiName" -}}
{{- $name := printf "%s-tei-%s" .root.Release.Name (.model.name | replace "." "-" | replace "_" "-") -}}
{{- if not (regexMatch "^[a-z]([-a-z0-9]{0,61}[a-z0-9])?$" $name) -}}
{{- fail (printf "model %q: %q is no valid Kubernetes name (DNS-1035 label of at most 63 characters)" .model.name $name) -}}
{{- end -}}
{{- $name -}}
{{- end }}

{{/* The server's config.yaml (FERRET_CONFIG_FILE). */}}
{{- define "ferret.config" -}}
embedding:
  default: {{ .Values.embedding.default | quote }}
  models:
    {{- range .Values.embedding.models }}
    - name: {{ .name | quote }}
      url: "http://{{ include "ferret.teiName" (dict "root" $ "model" .) }}:80"
      {{- with .queryPrefix }}
      query_prefix: {{ . | quote }}
      {{- end }}
      {{- with .documentPrefix }}
      document_prefix: {{ . | quote }}
      {{- end }}
    {{- end }}
{{- end }}
