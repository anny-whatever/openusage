import type { AppBootstrap, ProviderPresentation, Settings } from "@/platform/app-contract"

export function settingsFixture(overrides: Partial<Settings> = {}): Settings {
  return {
    schema: 3,
    enabledProviderIds: ["claude", "codex", "cursor"],
    knownProviderIds: providerIds,
    providerOrder: providerIds,
    metricLayouts: {},
    showTotalSpend: true,
    alwaysShowPacing: false,
    appearance: "system",
    density: "regular",
    meterStyle: "remaining",
    resetDisplay: "automatic",
    launchAtLogin: false,
    notifications: {
      underTenPercent: false,
      healthyToClose: false,
      closeToRunningOut: false,
    },
    shareAnonymousUsage: false,
    logLevel: "info",
    automaticallyCheckUpdates: true,
    betaUpdates: false,
    firstRunHintDismissed: false,
    ...overrides,
  }
}

export function bootstrapFixture(
  providers: ProviderPresentation[] = providerIds.map(providerFixture),
  settings = settingsFixture(),
): AppBootstrap {
  return {
    schema: "openusage.app-bootstrap.v1",
    providers,
    settings,
    capabilities: {
      refresh: false,
      screenshotExport: false,
      notifications: false,
      launchAtLogin: false,
      globalShortcut: false,
      externalLinks: false,
      commandLine: false,
      updates: false,
      logs: false,
    },
  }
}

export function providerFixture(id: string): ProviderPresentation {
  const displayName = id.charAt(0).toUpperCase() + id.slice(1)
  return {
    id,
    displayName,
    quickLink: `https://example.com/${id}`,
    supportsApiKey: id === "openrouter" || id === "zai",
    apiKeyStatus: id === "openrouter" || id === "zai" ? "missing" : undefined,
    apiKeyWarning: undefined,
  }
}

export const providerIds = [
  "claude",
  "codex",
  "cursor",
  "antigravity",
  "copilot",
  "devin",
  "grok",
  "opencode",
  "openrouter",
  "zai",
]
