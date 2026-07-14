import { useState } from "react"
import { ArrowDown, ArrowUp, ChevronRight, RotateCcw, Undo2 } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Switch } from "@/components/ui/switch"
import { MetricCustomization } from "@/features/customize/MetricCustomization"
import {
  moveProvider,
  resetAllLayout,
  setProviderEnabled,
} from "@/features/customize/customize-model"
import type { AppBootstrap, Settings } from "@/platform/app-contract"

type Props = {
  bootstrap: AppBootstrap
  canUndo: boolean
  onUndo: () => void
  onUpdate: (update: (settings: Settings) => Settings) => void
}
export function CustomizePage({ bootstrap, canUndo, onUndo, onUpdate }: Props) {
  const [selectedProviderId, setSelectedProviderId] = useState<string>()
  const providerById = new Map(bootstrap.providers.map((provider) => [provider.id, provider]))
  const selected = selectedProviderId ? providerById.get(selectedProviderId) : undefined

  if (selected) {
    return (
      <div className="mx-auto max-w-lg p-3">
        <MetricCustomization
          provider={selected}
          settings={bootstrap.settings}
          onBack={() => setSelectedProviderId(undefined)}
          onUpdate={onUpdate}
        />
      </div>
    )
  }

  return (
    <section aria-labelledby="customize-title" className="mx-auto max-w-lg space-y-3 p-3">
      <header className="flex items-center gap-2 px-1">
        <div className="min-w-0 flex-1">
          <h1 id="customize-title" className="text-lg font-semibold">Customize</h1>
          <p className="text-xs text-muted-foreground">Providers are shown in dashboard order</p>
        </div>
        <Button
          aria-label="Undo layout change"
          disabled={!canUndo}
          size="icon-sm"
          variant="ghost"
          onClick={onUndo}
        >
          <Undo2 aria-hidden="true" />
        </Button>
        <Button
          aria-label="Reset all layout"
          size="icon-sm"
          variant="ghost"
          onClick={() => onUpdate(resetAllLayout)}
        >
          <RotateCcw aria-hidden="true" />
        </Button>
      </header>

      {!bootstrap.settings.firstRunHintDismissed && (
        <Alert>
          <AlertTitle>Start With Providers</AlertTitle>
          <AlertDescription className="space-y-2">
            <p>Enable the accounts you use, then open each provider to arrange its metrics.</p>
            <Button
              size="sm"
              variant="outline"
              onClick={() =>
                onUpdate((settings) => ({ ...settings, firstRunHintDismissed: true }))
              }
            >
              Got It
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {bootstrap.settings.providerOrder.map((providerId, index) => {
        const provider = providerById.get(providerId)
        if (!provider) return null
        const enabled = bootstrap.settings.enabledProviderIds.includes(provider.id)
        return (
          <Card key={provider.id} className="py-0">
            <CardContent className="flex items-center gap-2 p-3">
              <Switch
                aria-label={`Enable ${provider.displayName}`}
                checked={enabled}
                onCheckedChange={(checked) =>
                  onUpdate((settings) => setProviderEnabled(settings, provider.id, checked))
                }
              />
              <button
                className="min-w-0 flex-1 rounded-md px-2 py-1 text-left focus-visible:ring-2 focus-visible:ring-ring"
                onClick={() => setSelectedProviderId(provider.id)}
              >
                <span className="block truncate text-sm font-medium">{provider.displayName}</span>
                <span className="block truncate text-xs text-muted-foreground">
                  {provider.snapshot?.lines.length ?? 0} available metrics
                </span>
              </button>
              <MoveButton
                providerName={provider.displayName}
                direction="up"
                disabled={index === 0}
                onClick={() =>
                  onUpdate((settings) => moveProvider(settings, provider.id, -1))
                }
              />
              <MoveButton
                providerName={provider.displayName}
                direction="down"
                disabled={index === bootstrap.settings.providerOrder.length - 1}
                onClick={() =>
                  onUpdate((settings) => moveProvider(settings, provider.id, 1))
                }
              />
              <Button
                aria-label={`Customize ${provider.displayName}`}
                size="icon-xs"
                variant="ghost"
                onClick={() => setSelectedProviderId(provider.id)}
              >
                <ChevronRight aria-hidden="true" />
              </Button>
            </CardContent>
          </Card>
        )
      })}
    </section>
  )
}

function MoveButton({
  providerName,
  direction,
  disabled,
  onClick,
}: {
  providerName: string
  direction: "up" | "down"
  disabled: boolean
  onClick: () => void
}) {
  const Icon = direction === "up" ? ArrowUp : ArrowDown
  return (
    <Button
      aria-label={`Move ${providerName} ${direction}`}
      disabled={disabled}
      size="icon-xs"
      variant="ghost"
      onClick={onClick}
    >
      <Icon aria-hidden="true" />
    </Button>
  )
}
