import { useState } from "react"
import { KeyRound, Trash2 } from "lucide-react"

import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { SettingsRow, SettingsSection } from "@/features/settings/SettingsControls"
import type { ProviderPresentation } from "@/platform/app-contract"

type ApiKeyAction = (providerId: string, key: string) => Promise<void>

export function ApiKeySettings({
  providers,
  onSave,
  onDelete,
}: {
  providers: ProviderPresentation[]
  onSave: ApiKeyAction
  onDelete: (providerId: string) => Promise<void>
}) {
  const supported = providers.filter((provider) => provider.supportsApiKey)
  return (
    <SettingsSection title="API Keys">
      {supported.map((provider) => (
        <ApiKeyRow key={provider.id} provider={provider} onSave={onSave} onDelete={onDelete} />
      ))}
    </SettingsSection>
  )
}
function ApiKeyRow({
  provider,
  onSave,
  onDelete,
}: {
  provider: ProviderPresentation
  onSave: ApiKeyAction
  onDelete: (providerId: string) => Promise<void>
}) {
  const [key, setKey] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const configured = provider.apiKeyStatus !== "missing"

  async function save() {
    setBusy(true)
    setError(undefined)
    try {
      await onSave(provider.id, key)
      setKey("")
    } catch {
      setError("The key could not be protected and saved.")
    } finally {
      setBusy(false)
    }
  }

  async function remove() {
    setBusy(true)
    setError(undefined)
    try {
      await onDelete(provider.id)
    } catch {
      setError("The stored key could not be removed.")
    } finally {
      setBusy(false)
    }
  }

  return (
    <div>
      <SettingsRow label={provider.displayName} detail={keyStatus(provider)}>
        <div className="flex items-center gap-1">
          <Input
            aria-label={`${provider.displayName} API key`}
            autoComplete="off"
            className="w-32"
            maxLength={16_384}
            placeholder="New key"
            type="password"
            value={key}
            onChange={(event) => setKey(event.target.value)}
          />
          <Button
            aria-label={`Save ${provider.displayName} API key`}
            disabled={busy || key.length === 0}
            size="icon-sm"
            onClick={save}
          >
            <KeyRound aria-hidden="true" />
          </Button>
          {configured && (
            <Button
              aria-label={`Delete ${provider.displayName} stored API key`}
              disabled={busy}
              size="icon-sm"
              variant="ghost"
              onClick={remove}
            >
              <Trash2 aria-hidden="true" />
            </Button>
          )}
        </div>
      </SettingsRow>
      {error && (
        <Alert variant="destructive" className="m-2">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
    </div>
  )
}

function keyStatus(provider: ProviderPresentation) {
  if (provider.apiKeyWarning) return provider.apiKeyWarning
  switch (provider.apiKeyStatus) {
    case "stored":
      return "Protected with Windows DPAPI"
    case "fromEnvironment":
      return "Using an external key"
    case "overrideActive":
      return "Stored key overrides an external key"
    default:
      return "No key configured"
  }
}
