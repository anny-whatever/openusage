import { ArrowLeft, CircleAlert, Info, LoaderCircle } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Separator } from "@/components/ui/separator"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"

type DesignSystemPageProps = {
  onClose: () => void
}

const buttonVariants = [
  "default",
  "secondary",
  "outline",
  "ghost",
  "link",
  "destructive",
] as const

const badgeVariants = ["default", "secondary", "outline", "ghost", "link", "destructive"] as const

export function DesignSystemPage({ onClose }: DesignSystemPageProps) {
  return (
    <main className="h-screen overflow-y-auto bg-background p-5 text-foreground">
      <section aria-labelledby="design-system-title" className="mx-auto flex max-w-md flex-col gap-5">
        <header className="flex items-center gap-3">
          <Button aria-label="Back to runtime status" size="icon-sm" variant="ghost" onClick={onClose}>
            <ArrowLeft aria-hidden="true" />
          </Button>
          <div>
            <p className="text-sm text-muted-foreground">Windows Foundation</p>
            <h1 id="design-system-title" className="text-xl font-semibold tracking-tight">
              Design System
            </h1>
          </div>
        </header>

        <FoundationCard />
        <ButtonCard />
        <FeedbackCard />
        <FormCard />
      </section>
    </main>
  )
}

function FoundationCard() {
  return (
    <Card className="gap-4 py-5">
      <CardHeader className="px-5">
        <CardTitle>Foundations</CardTitle>
      </CardHeader>
      <CardContent className="space-y-5 px-5">
        <div className="flex gap-2" aria-label="Color tokens">
          {[
            "bg-primary",
            "bg-secondary",
            "bg-accent",
            "bg-muted",
            "bg-destructive",
          ].map((colorClass) => (
            <span key={colorClass} className={`size-8 rounded-md border ${colorClass}`} />
          ))}
        </div>
        <div className="space-y-1">
          <p className="text-lg font-semibold">Segoe UI Variable</p>
          <p className="text-sm text-muted-foreground">Accessible Windows typography and spacing</p>
        </div>
        <div className="flex items-end gap-3" aria-label="Radius and shadow tokens">
          <span className="size-10 rounded-sm border bg-card shadow-sm" />
          <span className="size-12 rounded-md border bg-card shadow-md" />
          <span className="size-14 rounded-xl border bg-card shadow-lg" />
        </div>
      </CardContent>
    </Card>
  )
}

function ButtonCard() {
  return (
    <Card className="gap-4 py-5">
      <CardHeader className="px-5">
        <CardTitle>Actions and Status</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4 px-5">
        <div className="flex flex-wrap gap-2">
          {buttonVariants.map((variant) => (
            <Button key={variant} size="sm" variant={variant}>
              {variant}
            </Button>
          ))}
          <Button disabled size="sm">Disabled</Button>
        </div>
        <Separator />
        <div className="flex flex-wrap gap-2">
          {badgeVariants.map((variant) => (
            <Badge key={variant} variant={variant}>
              {variant}
            </Badge>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}

function FeedbackCard() {
  return (
    <Card className="gap-4 py-5">
      <CardHeader className="px-5">
        <CardTitle>Feedback States</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 px-5">
        <Alert>
          <Info aria-hidden="true" />
          <AlertTitle>Information</AlertTitle>
          <AlertDescription>Provider data is ready to refresh.</AlertDescription>
        </Alert>
        <Alert variant="destructive">
          <CircleAlert aria-hidden="true" />
          <AlertTitle>Error</AlertTitle>
          <AlertDescription>Credential access failed without exposing secret details.</AlertDescription>
        </Alert>
        <div className="flex items-center gap-3 text-sm text-muted-foreground" role="status">
          <LoaderCircle className="size-4 animate-spin" aria-hidden="true" />
          Loading provider status
        </div>
        <Skeleton className="h-9 w-full" />
      </CardContent>
    </Card>
  )
}

function FormCard() {
  return (
    <Card className="gap-4 py-5">
      <CardHeader className="px-5">
        <CardTitle>Form States</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4 px-5">
        <div className="space-y-2">
          <Label htmlFor="provider-name">Provider Name</Label>
          <Input id="provider-name" placeholder="OpenRouter" />
        </div>
        <div className="space-y-2">
          <Label htmlFor="invalid-key">Invalid Value</Label>
          <Input id="invalid-key" aria-invalid="true" defaultValue="Unavailable" />
        </div>
        <div className="flex items-center justify-between gap-4">
          <Label htmlFor="provider-enabled">Provider Enabled</Label>
          <Switch id="provider-enabled" defaultChecked />
        </div>
      </CardContent>
    </Card>
  )
}
