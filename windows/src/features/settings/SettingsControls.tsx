import type { ReactNode } from "react"

import { Card, CardContent } from "@/components/ui/card"
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select"
import { Switch } from "@/components/ui/switch"

export function SettingsSection({ title, children }: { title: string; children: ReactNode }) {
  const id = `settings-${title.toLocaleLowerCase().replaceAll(" ", "-")}`
  return (
    <section aria-labelledby={id} className="space-y-1.5">
      <h2 id={id} className="px-2 text-xs font-semibold text-muted-foreground">{title}</h2>
      <Card className="gap-0 py-0">
        <CardContent className="divide-y p-0">{children}</CardContent>
      </Card>
    </section>
  )
}

export function SettingsRow({ label, detail, children }: { label: string; detail?: string; children: ReactNode }) {
  return (
    <div className="flex min-h-11 items-center gap-3 px-3 py-2">
      <div className="min-w-0 flex-1">
        <p className="text-sm">{label}</p>
        {detail && <p className="text-xs text-muted-foreground">{detail}</p>}
      </div>
      {children}
    </div>
  )
}

export function SettingSwitch({ label, checked, disabled, onChange }: { label: string; checked: boolean; disabled?: boolean; onChange: (checked: boolean) => void }) {
  return (
    <Switch
      aria-label={label}
      checked={checked}
      disabled={disabled}
      onCheckedChange={onChange}
    />
  )
}

export function SettingSelect<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: readonly { value: T; label: string }[]; onChange: (value: T) => void }) {
  return (
    <NativeSelect
      aria-label={label}
      size="sm"
      value={value}
      onChange={(event) => onChange(event.target.value as T)}
    >
      {options.map((option) => (
        <NativeSelectOption key={option.value} value={option.value}>
          {option.label}
        </NativeSelectOption>
      ))}
    </NativeSelect>
  )
}
