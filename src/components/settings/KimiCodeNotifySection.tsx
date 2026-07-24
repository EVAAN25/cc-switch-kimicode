import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Play } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  kimiNotifyApi,
  type KimiNotifySettings,
} from "@/lib/api/kimi-notify";

const NO_SOUND = "none";

type NotifyEventKey = "stopSound" | "taskCompletedSound" | "subagentStopSound";

const EVENT_ROWS: { key: NotifyEventKey; labelKey: string }[] = [
  { key: "stopSound", labelKey: "settings.kimiNotify.events.stop" },
  {
    key: "taskCompletedSound",
    labelKey: "settings.kimiNotify.events.taskCompleted",
  },
  {
    key: "subagentStopSound",
    labelKey: "settings.kimiNotify.events.subagentStop",
  },
];

export function KimiCodeNotifySection() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<KimiNotifySettings | null>(null);
  const [sounds, setSounds] = useState<string[]>([]);

  useEffect(() => {
    kimiNotifyApi
      .getSettings()
      .then(setSettings)
      .catch((e) => console.error("Failed to load kimi notify settings:", e));
    kimiNotifyApi
      .listSounds()
      .then(setSounds)
      .catch((e) => console.error("Failed to list kimi notify sounds:", e));
  }, []);

  const handleChange = async (updates: Partial<KimiNotifySettings>) => {
    if (!settings) return;
    const next = { ...settings, ...updates };
    setSettings(next);
    try {
      await kimiNotifyApi.setSettings(next);
    } catch (e) {
      console.error("Failed to save kimi notify settings:", e);
      toast.error(String(e));
      setSettings(settings);
    }
  };

  const handlePreview = async (name: string) => {
    try {
      await kimiNotifyApi.preview(name);
    } catch (e) {
      console.error("Failed to preview kimi notify sound:", e);
      toast.error(String(e));
    }
  };

  if (!settings) return null;

  return (
    <section className="space-y-3">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.kimiNotify.title")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.kimiNotify.description")}
        </p>
      </header>

      <div className="flex items-center justify-between">
        <Label>{t("settings.kimiNotify.enabled")}</Label>
        <Switch
          checked={settings.enabled}
          onCheckedChange={(checked) => handleChange({ enabled: checked })}
        />
      </div>

      {EVENT_ROWS.map(({ key, labelKey }) => {
        const value = settings[key] ?? NO_SOUND;
        return (
          <div key={key} className="flex items-center justify-between gap-3">
            <Label className="shrink-0">{t(labelKey)}</Label>
            <div className="flex items-center gap-2">
              <Select
                value={value}
                disabled={!settings.enabled}
                onValueChange={(selected) =>
                  handleChange({
                    [key]: selected === NO_SOUND ? null : selected,
                  })
                }
              >
                <SelectTrigger className="w-[140px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NO_SOUND}>
                    {t("settings.kimiNotify.noSound")}
                  </SelectItem>
                  {sounds.map((name) => (
                    <SelectItem key={name} value={name}>
                      {name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                disabled={!settings.enabled || value === NO_SOUND}
                onClick={() => void handlePreview(value)}
                title={t("settings.kimiNotify.preview")}
              >
                <Play className="h-4 w-4" />
              </Button>
            </div>
          </div>
        );
      })}
    </section>
  );
}
