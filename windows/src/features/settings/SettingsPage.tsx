import {
  Blocks,
  ExternalLink,
  FolderOpen,
  Keyboard,
  RefreshCw,
  TerminalSquare,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { ApiKeySettings } from "@/features/settings/ApiKeySettings"
import {
  SettingSelect,
  SettingSwitch,
  SettingsRow,
  SettingsSection,
} from "@/features/settings/SettingsControls"
import type { AppBootstrap, Settings } from "@/platform/app-contract"

type Props = {
  bootstrap: AppBootstrap
  onUpdate: (update: (settings: Settings) => Settings) => void
  onSaveApiKey: (providerId: string, key: string) => Promise<void>
  onDeleteApiKey: (providerId: string) => Promise<void>
  onOpenDesignSystem: () => void
}

export function SettingsPage({
  bootstrap,
  onUpdate,
  onSaveApiKey,
  onDeleteApiKey,
  onOpenDesignSystem,
}: Props) {
  const { settings, capabilities } = bootstrap
  return (
    <section aria-labelledby="settings-title" className="mx-auto max-w-lg space-y-4 p-3 pb-6">
      <header className="px-1">
        <h1 id="settings-title" className="text-lg font-semibold">Settings</h1>
        <p className="text-xs text-muted-foreground">Windows preferences and privacy</p>
      </header>

      <SettingsSection title="General">
        <SettingsRow label="Show Total Spend">
          <SettingSwitch
            label="Show Total Spend"
            checked={settings.showTotalSpend}
            onChange={(showTotalSpend) => onUpdate((value) => ({ ...value, showTotalSpend }))}
          />
        </SettingsRow>
        <SettingsRow
          label="Launch at Login"
          detail={capabilities.launchAtLogin ? undefined : "Available with shell integration in P6"}
        >
          <SettingSwitch
            label="Launch at Login"
            checked={settings.launchAtLogin}
            disabled={!capabilities.launchAtLogin}
            onChange={(launchAtLogin) => onUpdate((value) => ({ ...value, launchAtLogin }))}
          />
        </SettingsRow>
        <SettingsRow label="Global Shortcut" detail="Available with shell integration in P6">
          <Button
            aria-label="Record global shortcut"
            disabled={!capabilities.globalShortcut}
            size="icon-sm"
            variant="outline"
          >
            <Keyboard aria-hidden="true" />
          </Button>
        </SettingsRow>
      </SettingsSection>

      <SettingsSection title="Appearance">
        <SettingsRow label="Theme">
          <SettingSelect
            label="Theme"
            value={settings.appearance}
            options={[
              { value: "system", label: "System" },
              { value: "light", label: "Light" },
              { value: "dark", label: "Dark" },
            ]}
            onChange={(appearance) => onUpdate((value) => ({ ...value, appearance }))}
          />
        </SettingsRow>
        <SettingsRow label="Density">
          <SettingSelect
            label="Density"
            value={settings.density}
            options={[
              { value: "regular", label: "Regular" },
              { value: "compact", label: "Compact" },
            ]}
            onChange={(density) => onUpdate((value) => ({ ...value, density }))}
          />
        </SettingsRow>
        <SettingsRow label="Component Catalog">
          <Button
            aria-label="Open design system"
            size="icon-sm"
            variant="ghost"
            onClick={onOpenDesignSystem}
          >
            <Blocks aria-hidden="true" />
          </Button>
        </SettingsRow>
      </SettingsSection>

      <UsageDisplaySettings settings={settings} onUpdate={onUpdate} />
      <NotificationSettings bootstrap={bootstrap} onUpdate={onUpdate} />
      <ApiKeySettings
        providers={bootstrap.providers}
        onSave={onSaveApiKey}
        onDelete={onDeleteApiKey}
      />

      <SettingsSection title="Privacy">
        <SettingsRow
          label="Share Anonymous Usage"
          detail="Only coarse counts and error types; never credentials or usage values"
        >
          <SettingSwitch
            label="Share Anonymous Usage"
            checked={settings.shareAnonymousUsage}
            onChange={(shareAnonymousUsage) =>
              onUpdate((value) => ({ ...value, shareAnonymousUsage }))
            }
          />
        </SettingsRow>
      </SettingsSection>

      <SettingsSection title="Logging">
        <SettingsRow label="Log Level">
          <SettingSelect
            label="Log Level"
            value={settings.logLevel}
            options={[
              { value: "error", label: "Errors" },
              { value: "info", label: "Info" },
              { value: "debug", label: "Debug" },
            ]}
            onChange={(logLevel) => onUpdate((value) => ({ ...value, logLevel }))}
          />
        </SettingsRow>
        <SettingsRow label="Open Logs" detail="Available with shell integration in P6">
          <Button
            aria-label="Open logs folder"
            disabled={!capabilities.logs}
            size="icon-sm"
            variant="outline"
          >
            <FolderOpen aria-hidden="true" />
          </Button>
        </SettingsRow>
      </SettingsSection>

      <PlatformSettings bootstrap={bootstrap} onUpdate={onUpdate} />
    </section>
  )
}

function UsageDisplaySettings({ settings, onUpdate }: { settings: Settings; onUpdate: Props["onUpdate"] }) {
  return (
    <SettingsSection title="Usage Display">
      <SettingsRow label="Show Usage As">
        <SettingSelect
          label="Show Usage As"
          value={settings.meterStyle}
          options={[
            { value: "remaining", label: "Remaining" },
            { value: "used", label: "Used" },
          ]}
          onChange={(meterStyle) => onUpdate((value) => ({ ...value, meterStyle }))}
        />
      </SettingsRow>
      <SettingsRow label="Reset Times">
        <SettingSelect
          label="Reset Times"
          value={settings.resetDisplay}
          options={[
            { value: "automatic", label: "Automatic" },
            { value: "countdown", label: "Countdown" },
            { value: "time", label: "Clock Time" },
          ]}
          onChange={(resetDisplay) => onUpdate((value) => ({ ...value, resetDisplay }))}
        />
      </SettingsRow>
      <SettingsRow label="Always Show Pacing">
        <SettingSwitch
          label="Always Show Pacing"
          checked={settings.alwaysShowPacing}
          onChange={(alwaysShowPacing) =>
            onUpdate((value) => ({ ...value, alwaysShowPacing }))
          }
        />
      </SettingsRow>
    </SettingsSection>
  )
}

function NotificationSettings({ bootstrap, onUpdate }: { bootstrap: AppBootstrap; onUpdate: Props["onUpdate"] }) {
  const notifications = bootstrap.settings.notifications
  const disabled = !bootstrap.capabilities.notifications
  const setNotification = (name: keyof Settings["notifications"], checked: boolean) =>
    onUpdate((value) => ({
      ...value,
      notifications: { ...value.notifications, [name]: checked },
    }))
  return (
    <SettingsSection title="Notifications">
      <SettingsRow
        label="Under 10% Remaining"
        detail={disabled ? "Delivery arrives with shell integration in P6" : undefined}
      >
        <SettingSwitch label="Under 10% Remaining" checked={notifications.underTenPercent} disabled={disabled} onChange={(checked) => setNotification("underTenPercent", checked)} />
      </SettingsRow>
      <SettingsRow label="Healthy to Close">
        <SettingSwitch label="Healthy to Close" checked={notifications.healthyToClose} disabled={disabled} onChange={(checked) => setNotification("healthyToClose", checked)} />
      </SettingsRow>
      <SettingsRow label="Close to Running Out">
        <SettingSwitch label="Close to Running Out" checked={notifications.closeToRunningOut} disabled={disabled} onChange={(checked) => setNotification("closeToRunningOut", checked)} />
      </SettingsRow>
    </SettingsSection>
  )
}

function PlatformSettings({ bootstrap, onUpdate }: { bootstrap: AppBootstrap; onUpdate: Props["onUpdate"] }) {
  const { capabilities, settings } = bootstrap
  return (
    <>
      <SettingsSection title="Command Line">
        <SettingsRow label="Terminal Helper" detail="Available with local API and CLI integration in P6">
          <Button disabled={!capabilities.commandLine} size="sm" variant="outline">
            <TerminalSquare aria-hidden="true" />Install
          </Button>
        </SettingsRow>
      </SettingsSection>
      <SettingsSection title="Updates">
        <SettingsRow label="Update Automatically">
          <SettingSwitch label="Update Automatically" checked={settings.automaticallyCheckUpdates} disabled={!capabilities.updates} onChange={(automaticallyCheckUpdates) => onUpdate((value) => ({ ...value, automaticallyCheckUpdates }))} />
        </SettingsRow>
        <SettingsRow label="Beta Updates">
          <SettingSwitch label="Beta Updates" checked={settings.betaUpdates} disabled={!capabilities.updates} onChange={(betaUpdates) => onUpdate((value) => ({ ...value, betaUpdates }))} />
        </SettingsRow>
        <SettingsRow label="Check for Updates" detail="Enabled after signed Windows packaging in P8">
          <Button aria-label="Check for updates" disabled={!capabilities.updates} size="icon-sm" variant="outline"><RefreshCw aria-hidden="true" /></Button>
        </SettingsRow>
      </SettingsSection>
      <SettingsSection title="Help">
        <SettingsRow label="Windows Documentation" detail="External links arrive with shell integration in P6">
          <Button aria-label="Open Windows documentation" disabled={!capabilities.externalLinks} size="icon-sm" variant="ghost"><ExternalLink aria-hidden="true" /></Button>
        </SettingsRow>
      </SettingsSection>
    </>
  )
}
