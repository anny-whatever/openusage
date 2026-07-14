export type ErrorCategory =
  | "not_logged_in"
  | "auth_expired"
  | "auth_invalid"
  | "credential_access"
  | "network"
  | "decoding"
  | "http_4xx"
  | "http_5xx"
  | "rate_limited"
  | "not_available"
  | "other"

export type MetricValue = {
  number: number
  kind: "percent" | "dollars" | "count"
  label?: string
  estimated: boolean
}

export type MetricLine =
  | { type: "text"; label: string; value: string; colorHex?: string; subtitle?: string }
  | {
      type: "values"
      label: string
      values: MetricValue[]
      colorHex?: string
      expiriesAt?: string[]
      unknownModels?: string[]
    }
  | {
      type: "progress"
      label: string
      used: number
      limit: number
      format: "percent" | "dollars" | { kind: "count"; suffix: string }
      resetsAt?: string
      periodDurationMs?: number
      colorHex?: string
    }
  | { type: "badge"; label: string; text: string; colorHex?: string; subtitle?: string }
  | {
      type: "chart"
      label: string
      points: { value: number; label: string; valueLabel?: string }[]
      note?: string
    }

export type ProviderSnapshot = {
  providerId: string
  displayName: string
  plan?: string
  lines: MetricLine[]
  refreshedAt: string
  usageHistory?: { days: { date: string; value: number }[] }
  warning?: string
  errorCategory?: ErrorCategory
}

export type MetricLayout = {
  orderedMetricIds: string[]
  hiddenMetricIds: string[]
  onDemandMetricIds: string[]
  starredMetricIds: string[]
}

export type Settings = {
  schema: 3
  enabledProviderIds: string[]
  knownProviderIds: string[]
  providerOrder: string[]
  metricLayouts: Record<string, MetricLayout>
  showTotalSpend: boolean
  alwaysShowPacing: boolean
  appearance: "system" | "light" | "dark"
  density: "regular" | "compact"
  meterStyle: "used" | "remaining"
  resetDisplay: "automatic" | "countdown" | "time"
  launchAtLogin: boolean
  notifications: {
    underTenPercent: boolean
    healthyToClose: boolean
    closeToRunningOut: boolean
  }
  shareAnonymousUsage: boolean
  logLevel: "error" | "info" | "debug"
  automaticallyCheckUpdates: boolean
  betaUpdates: boolean
  firstRunHintDismissed: boolean
}

export type ApiKeyStatus = "missing" | "fromEnvironment" | "stored" | "overrideActive"

export type ProviderPresentation = {
  id: string
  displayName: string
  quickLink: string
  supportsApiKey: boolean
  apiKeyStatus?: ApiKeyStatus
  apiKeyWarning?: string
  snapshot?: ProviderSnapshot
}

export type PlatformCapabilities = {
  refresh: boolean
  screenshotExport: boolean
  notifications: boolean
  launchAtLogin: boolean
  globalShortcut: boolean
  externalLinks: boolean
  commandLine: boolean
  updates: boolean
  logs: boolean
}

export type AppBootstrap = {
  schema: "openusage.app-bootstrap.v1"
  settings: Settings
  providers: ProviderPresentation[]
  capabilities: PlatformCapabilities
}

export function metricId(providerId: string, line: MetricLine): string {
  const label = line.label
    .toLocaleLowerCase("en-US")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
  return `${providerId}:${label || "metric"}`
}
