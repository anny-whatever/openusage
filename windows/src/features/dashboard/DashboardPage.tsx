import { CircleAlert, RefreshCw, SlidersHorizontal, WalletCards, X } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { enabledProviders, totalSpend } from "@/features/dashboard/dashboard-model"
import { ProviderCard } from "@/features/dashboard/ProviderCard"
import type { AppBootstrap } from "@/platform/app-contract"

export function DashboardPage({
  bootstrap,
  error,
  loading,
  onCustomize,
  onDismissHint,
  onReload,
}: {
  bootstrap?: AppBootstrap
  error?: string
  loading: boolean
  onCustomize: () => void
  onDismissHint: () => void
  onReload: () => void
}) {
  if (loading && !bootstrap) return <DashboardLoading />
  if (error && !bootstrap) return <DashboardError message={error} onReload={onReload} />
  if (!bootstrap) return null
  const providers = enabledProviders(bootstrap.providers, bootstrap.settings)
  const spend = totalSpend(bootstrap.providers, bootstrap.settings)

  return (
    <section aria-labelledby="dashboard-title" className="mx-auto flex max-w-lg flex-col gap-3 p-3">
      <header className="flex items-center justify-between gap-3 px-1">
        <div><h1 id="dashboard-title" className="text-lg font-semibold">Dashboard</h1><p className="text-xs text-muted-foreground">Your AI usage at a glance</p></div>
        <Button aria-label="Reload cached usage" size="icon-sm" variant="ghost" onClick={onReload}><RefreshCw aria-hidden="true" /></Button>
      </header>
      {!bootstrap.settings.firstRunHintDismissed && (
        <Alert><SlidersHorizontal aria-hidden="true" /><AlertTitle>Make It Yours</AlertTitle><AlertDescription className="flex items-start gap-2"><span className="flex-1">Use Customize to choose providers, details, and starred tray metrics.</span><Button aria-label="Dismiss first-run hint" size="icon-xs" variant="ghost" onClick={onDismissHint}><X aria-hidden="true" /></Button></AlertDescription></Alert>
      )}
      {bootstrap.settings.showTotalSpend && spend !== undefined && (
        <Card><CardContent className="flex items-center gap-3 p-4"><WalletCards className="size-5 text-primary" aria-hidden="true" /><div><p className="text-xs text-muted-foreground">Total Spend</p><p className="text-xl font-semibold">{new Intl.NumberFormat(undefined, { style: "currency", currency: "USD" }).format(spend)}</p></div></CardContent></Card>
      )}
      {providers.length === 0 ? (
        <Card><CardContent className="flex flex-col items-center gap-3 p-6 text-center"><p className="font-medium">No Providers Enabled</p><p className="text-sm text-muted-foreground">Choose at least one provider to build your dashboard.</p><Button onClick={onCustomize}>Choose Providers</Button></CardContent></Card>
      ) : providers.map((provider) => (
        <ProviderCard key={provider.id} capabilities={bootstrap.capabilities} provider={provider} settings={bootstrap.settings} />
      ))}
      {!bootstrap.capabilities.refresh && <p className="px-1 text-center text-xs text-muted-foreground">Live refresh arrives with Windows shell integration in P6; cached snapshots remain local.</p>}
    </section>
  )
}

function DashboardLoading() {
  return <section aria-label="Loading dashboard" className="mx-auto max-w-lg space-y-3 p-3" role="status"><span className="sr-only">Loading usage</span><Skeleton className="h-12" /><Skeleton className="h-32" /><Skeleton className="h-32" /></section>
}

function DashboardError({ message, onReload }: { message: string; onReload: () => void }) {
  return <section className="p-4"><Alert variant="destructive"><CircleAlert aria-hidden="true" /><AlertTitle>Windows Service Unavailable</AlertTitle><AlertDescription className="space-y-3"><p>{message}</p><Button size="sm" variant="outline" onClick={onReload}>Try Again</Button></AlertDescription></Alert></section>
}
