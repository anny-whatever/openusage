import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import type { PlatformCapabilities, ProviderPresentation } from "@/platform/app-contract"

export function ShareDialog({
  provider,
  capabilities,
  open,
  onOpenChange,
}: {
  provider: ProviderPresentation
  capabilities: PlatformCapabilities
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Share {provider.displayName}</DialogTitle>
          <DialogDescription>The share card contains normalized usage only—never credentials or raw provider responses.</DialogDescription>
        </DialogHeader>
        <div className="rounded-xl border bg-card p-4 shadow-sm" aria-label="Share card preview">
          <p className="font-semibold">OpenUsage · {provider.displayName}</p>
          <p className="mt-1 text-sm text-muted-foreground">{provider.snapshot?.lines.length ?? 0} usage metrics</p>
        </div>
        <Button disabled={!capabilities.screenshotExport}>Copy Share Image</Button>
        {!capabilities.screenshotExport && <p className="text-xs text-muted-foreground">Native Windows image export is installed with shell integration in P6.</p>}
      </DialogContent>
    </Dialog>
  )
}
