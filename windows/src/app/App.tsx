import { useEffect, useState } from "react"

import { AppErrorBoundary } from "@/app/AppErrorBoundary"
import { AppShell, type AppPage } from "@/app/AppShell"
import { DesignSystemPage } from "@/app/DesignSystemPage"
import { CustomizePage } from "@/features/customize/CustomizePage"
import { DashboardPage } from "@/features/dashboard/DashboardPage"
import { SettingsPage } from "@/features/settings/SettingsPage"
import { useOpenUsage } from "@/features/app/use-openusage"

export function App() {
  const [page, setPage] = useState<AppPage>("dashboard")
  const model = useOpenUsage()

  useAppearance(model.bootstrap?.settings.appearance)
  useDensity(model.bootstrap?.settings.density)

  if (page === "design-system") {
    return <DesignSystemPage onClose={() => setPage("settings")} />
  }

  return (
    <AppErrorBoundary>
      <AppShell page={page} saving={model.saving} onNavigate={setPage}>
        {page === "dashboard" && (
          <DashboardPage
            bootstrap={model.bootstrap}
            error={model.error}
            loading={model.loading}
            onCustomize={() => setPage("customize")}
            onDismissHint={() => model.updateSettings((settings) => ({ ...settings, firstRunHintDismissed: true }))}
            onReload={model.reload}
          />
        )}
        {page === "customize" && model.bootstrap && (
          <CustomizePage
            bootstrap={model.bootstrap}
            canUndo={model.canUndo}
            onUndo={model.undo}
            onUpdate={model.updateSettings}
          />
        )}
        {page === "settings" && model.bootstrap && (
          <SettingsPage
            bootstrap={model.bootstrap}
            onDeleteApiKey={model.deleteApiKey}
            onOpenDesignSystem={() => setPage("design-system")}
            onSaveApiKey={model.saveApiKey}
            onUpdate={model.updateSettings}
          />
        )}
      </AppShell>
    </AppErrorBoundary>
  )
}

function useAppearance(appearance?: "system" | "light" | "dark") {
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)")
    const apply = () => {
      const dark = appearance === "dark" || (appearance === "system" && media.matches)
      document.documentElement.classList.toggle("dark", dark)
      document.documentElement.style.colorScheme = dark ? "dark" : "light"
    }
    apply()
    media.addEventListener("change", apply)
    return () => media.removeEventListener("change", apply)
  }, [appearance])
}

function useDensity(density?: "regular" | "compact") {
  useEffect(() => {
    document.documentElement.dataset.density = density ?? "regular"
  }, [density])
}
