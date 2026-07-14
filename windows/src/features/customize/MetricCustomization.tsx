import { useId } from "react"
import { ArrowDown, ArrowLeft, ArrowUp, Eye, EyeOff, Star } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Switch } from "@/components/ui/switch"
import {
  moveMetric,
  providerMetricIds,
  resetProvider,
  setMetricHidden,
  setMetricOnDemand,
  setMetricStarred,
} from "@/features/customize/customize-model"
import { layoutFor } from "@/features/dashboard/dashboard-model"
import type { ProviderPresentation, Settings } from "@/platform/app-contract"

export function MetricCustomization({
  provider,
  settings,
  onBack,
  onUpdate,
}: {
  provider: ProviderPresentation
  settings: Settings
  onBack: () => void
  onUpdate: (update: (settings: Settings) => Settings) => void
}) {
  const ids = providerMetricIds(provider)
  const layout = layoutFor(settings, provider.id)
  const orderedIds = [...layout.orderedMetricIds.filter((id) => ids.includes(id)), ...ids.filter((id) => !layout.orderedMetricIds.includes(id))]
  const lines = new Map((provider.snapshot?.lines ?? []).map((line, index) => [ids[index], line]))

  return (
    <section aria-labelledby="metric-customization-title" className="space-y-3">
      <header className="flex items-center gap-2">
        <Button aria-label="Back to providers" size="icon-sm" variant="ghost" onClick={onBack}><ArrowLeft aria-hidden="true" /></Button>
        <div className="min-w-0 flex-1"><h2 id="metric-customization-title" className="truncate font-semibold">{provider.displayName}</h2><p className="text-xs text-muted-foreground">Choose visibility, details, stars, and order</p></div>
        <Button size="sm" variant="ghost" onClick={() => onUpdate((value) => resetProvider(value, provider.id))}>Reset</Button>
      </header>
      {orderedIds.length === 0 && <Card><CardContent className="p-5 text-sm text-muted-foreground">Metrics appear here after the first successful provider refresh.</CardContent></Card>}
      {orderedIds.map((id, index) => {
        const line = lines.get(id)
        if (!line) return null
        const hidden = layout.hiddenMetricIds.includes(id)
        const onDemand = layout.onDemandMetricIds.includes(id)
        const starred = layout.starredMetricIds.includes(id)
        return (
          <Card key={id} className="gap-2 py-3">
            <CardHeader className="flex-row items-center gap-2 px-3">
              <CardTitle className="min-w-0 flex-1 truncate text-sm">{line.label}</CardTitle>
              <Button aria-label={`Move ${line.label} up`} disabled={index === 0} size="icon-xs" variant="ghost" onClick={() => onUpdate((value) => moveMetric(value, provider, id, -1))}><ArrowUp aria-hidden="true" /></Button>
              <Button aria-label={`Move ${line.label} down`} disabled={index === orderedIds.length - 1} size="icon-xs" variant="ghost" onClick={() => onUpdate((value) => moveMetric(value, provider, id, 1))}><ArrowDown aria-hidden="true" /></Button>
            </CardHeader>
            <CardContent className="grid grid-cols-3 gap-2 px-3 text-xs">
              <Control label={hidden ? "Hidden" : "Visible"} icon={hidden ? EyeOff : Eye} checked={!hidden} onChange={(checked) => onUpdate((value) => setMetricHidden(value, provider, id, !checked))} />
              <Control label={onDemand ? "On Demand" : "Always"} icon={Eye} checked={!onDemand} disabled={hidden} onChange={(checked) => onUpdate((value) => setMetricOnDemand(value, provider, id, !checked))} />
              <Control label="Starred" icon={Star} checked={starred} disabled={hidden} onChange={(checked) => onUpdate((value) => setMetricStarred(value, provider, id, checked))} />
            </CardContent>
          </Card>
        )
      })}
    </section>
  )
}

function Control({ label, icon: Icon, checked, disabled, onChange }: { label: string; icon: typeof Eye; checked: boolean; disabled?: boolean; onChange: (checked: boolean) => void }) {
  const id = useId()
  return <label className="flex flex-col items-center gap-1.5 rounded-md border p-2 text-center"><Icon className="size-3.5" aria-hidden="true" /><span>{label}</span><Switch aria-label={label} id={id} checked={checked} disabled={disabled} onCheckedChange={onChange} /></label>
}
