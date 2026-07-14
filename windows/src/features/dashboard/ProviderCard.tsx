import { useState } from "react"
import { ChevronDown, ExternalLink, RotateCcw, Share2, TriangleAlert } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from "@/components/ui/alert-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"
import { MetricLineView } from "@/features/dashboard/MetricLineView"
import { isStale, layoutFor, orderedLines } from "@/features/dashboard/dashboard-model"
import { ShareDialog } from "@/features/dashboard/ShareDialog"
import type { PlatformCapabilities, ProviderPresentation, Settings } from "@/platform/app-contract"

export function ProviderCard({
  provider,
  settings,
  capabilities,
}: {
  provider: ProviderPresentation
  settings: Settings
  capabilities: PlatformCapabilities
}) {
  const [expanded, setExpanded] = useState(false)
  const [sharing, setSharing] = useState(false)
  const lines = orderedLines(provider, settings)
  const layout = layoutFor(settings, provider.id)
  const visible = lines.filter(({ id }) => !layout.onDemandMetricIds.includes(id))
  const onDemand = lines.filter(({ id }) => layout.onDemandMetricIds.includes(id))
  const stale = provider.snapshot ? isStale(provider.snapshot.refreshedAt) : false

  return (
    <Card className="gap-0 overflow-hidden py-0">
      <CardHeader className="flex flex-row items-start gap-3 px-4 py-3">
        <div className="min-w-0 flex-1">
          <CardTitle className="flex flex-wrap items-center gap-2 text-base">
            {provider.displayName}
            {provider.snapshot?.plan && <Badge variant="secondary">{provider.snapshot.plan}</Badge>}
            {stale && <Badge variant="outline">Stale</Badge>}
          </CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">{providerStatus(provider)}</p>
        </div>
        {provider.snapshot && (
          <Button
            aria-label={`Share ${provider.displayName}`}
            size="icon-xs"
            variant="ghost"
            onClick={() => setSharing(true)}
          >
            <Share2 aria-hidden="true" />
          </Button>
        )}
      </CardHeader>
      <CardContent className="px-4 pb-3">
        {!provider.snapshot && (
          <>
            <UnauthorizedState provider={provider} />
            <QuickLinkButton enabled={capabilities.externalLinks} />
          </>
        )}
        {provider.snapshot?.errorCategory && <ProviderError category={provider.snapshot.errorCategory} />}
        {provider.snapshot?.warning && (
          <Alert className="mb-2"><TriangleAlert aria-hidden="true" /><AlertTitle>Partial Data</AlertTitle><AlertDescription>{provider.snapshot.warning}</AlertDescription></Alert>
        )}
        {visible.map(({ id, line }) => <MetricLineView key={id} line={line} alwaysShowPacing={settings.alwaysShowPacing} meterStyle={settings.meterStyle} resetDisplay={settings.resetDisplay} />)}
        {provider.snapshot && lines.length === 0 && <p className="py-3 text-sm text-muted-foreground">No enabled metrics have data yet.</p>}
        {provider.snapshot && (
          <Collapsible open={expanded} onOpenChange={setExpanded}>
            <CollapsibleTrigger asChild>
              <Button className="mt-1 w-full" size="sm" variant="ghost">
                <ChevronDown className={expanded ? "rotate-180" : undefined} aria-hidden="true" />
                {expanded ? "Hide Details" : onDemand.length > 0 ? `Show ${onDemand.length} More` : "Show Links"}
              </Button>
            </CollapsibleTrigger>
            <CollapsibleContent>
              {onDemand.map(({ id, line }) => <MetricLineView key={id} line={line} alwaysShowPacing={settings.alwaysShowPacing} meterStyle={settings.meterStyle} resetDisplay={settings.resetDisplay} />)}
              <div className="flex flex-wrap gap-2 pt-2">
                <QuickLinkButton enabled={capabilities.externalLinks} />
                {hasResetCredits(provider) && <ResetCreditConfirmation enabled={capabilities.refresh} />}
              </div>
            </CollapsibleContent>
          </Collapsible>
        )}
      </CardContent>
      <ShareDialog capabilities={capabilities} open={sharing} provider={provider} onOpenChange={setSharing} />
    </Card>
  )
}

function QuickLinkButton({ enabled }: { enabled: boolean }) {
  return (
    <Button disabled={!enabled} size="sm" variant="outline">
      <ExternalLink aria-hidden="true" />Provider Page{enabled ? "" : " · P6"}
    </Button>
  )
}

function hasResetCredits(provider: ProviderPresentation) {
  return provider.id === "codex" && provider.snapshot?.lines.some((line) => line.type === "values" && line.label === "Rate Limit Resets")
}

function ResetCreditConfirmation({ enabled }: { enabled: boolean }) {
  return <AlertDialog><AlertDialogTrigger asChild><Button size="sm" variant="outline"><RotateCcw aria-hidden="true" />Claim Reset</Button></AlertDialogTrigger><AlertDialogContent><AlertDialogHeader><AlertDialogTitle>Claim a Codex Reset?</AlertDialogTitle><AlertDialogDescription>This consumes one matching reset credit and refreshes Codex usage. The action is idempotent, but it cannot be undone.</AlertDialogDescription></AlertDialogHeader>{!enabled && <p className="text-xs text-muted-foreground">Reset claiming is enabled when the live provider coordinator lands in P6.</p>}<AlertDialogFooter><AlertDialogCancel>Cancel</AlertDialogCancel><AlertDialogAction disabled={!enabled}>Claim Reset</AlertDialogAction></AlertDialogFooter></AlertDialogContent></AlertDialog>
}

function UnauthorizedState({ provider }: { provider: ProviderPresentation }) {
  const detail = provider.supportsApiKey
    ? "Add an API key in Settings to load usage."
    : "Sign in with the provider's Windows app or CLI, then refresh."
  return <p className="py-3 text-sm text-muted-foreground">{detail}</p>
}

function ProviderError({ category }: { category: string }) {
  return (
    <Alert variant="destructive" className="mb-2">
      <TriangleAlert aria-hidden="true" />
      <AlertTitle>Provider Unavailable</AlertTitle>
      <AlertDescription>{friendlyCategory(category)}</AlertDescription>
    </Alert>
  )
}

function providerStatus(provider: ProviderPresentation) {
  if (!provider.snapshot) return "Waiting for account access"
  if (provider.snapshot.errorCategory) return "Last refresh failed"
  return `Updated ${new Date(provider.snapshot.refreshedAt).toLocaleString()}`
}

function friendlyCategory(category: string) {
  if (category.includes("auth") || category === "not_logged_in") return "OpenUsage could not use this account. Check the provider sign-in."
  if (category === "network" || category === "rate_limited") return "The provider could not be reached. Your last safe snapshot remains visible."
  return "This provider returned data OpenUsage could not safely display."
}
