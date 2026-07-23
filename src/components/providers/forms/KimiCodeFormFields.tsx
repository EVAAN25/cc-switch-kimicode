import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import ApiKeyInput from "./ApiKeyInput";
import {
  KIMI_CODE_DEFAULT_BASE_URL,
  KIMI_CODE_DEFAULT_MODEL,
  KIMI_CODE_MODEL_OPTIONS,
  applyKimiCodeFields,
  parseKimiCodeFields,
  type KimiCodeFormFieldsData,
} from "@/config/kimiCode";

interface KimiCodeFormFieldsProps {
  /** 当前完整 TOML（来自表单 settingsConfig）。 */
  toml: string;
  onTomlChange: (toml: string) => void;
}

/** Kimi Code 供应商的结构化表单：API Key / Base URL / 模型，
 * 高级区保留整段 TOML 编辑器，两边双向同步。 */
export function KimiCodeFormFields({
  toml,
  onTomlChange,
}: KimiCodeFormFieldsProps) {
  const { t } = useTranslation();
  // 记录本组件最近一次发出的 TOML，用于区分外部变化与自身回环
  const lastTomlRef = useRef(toml);
  const [fields, setFields] = useState<KimiCodeFormFieldsData>(
    () =>
      parseKimiCodeFields(toml) ?? {
        apiKey: "",
        baseUrl: KIMI_CODE_DEFAULT_BASE_URL,
        model: KIMI_CODE_DEFAULT_MODEL,
      },
  );
  const [advancedOpen, setAdvancedOpen] = useState(false);

  // 外部重置/切换供应商时，从最新 TOML 重新解析回填字段
  useEffect(() => {
    if (toml === lastTomlRef.current) return;
    lastTomlRef.current = toml;
    const parsed = parseKimiCodeFields(toml);
    if (parsed) setFields(parsed);
  }, [toml]);

  const updateFields = (patch: Partial<KimiCodeFormFieldsData>) => {
    const next = { ...fields, ...patch };
    setFields(next);
    const nextToml = applyKimiCodeFields(lastTomlRef.current, next);
    lastTomlRef.current = nextToml;
    onTomlChange(nextToml);
  };

  // 高级区手改 TOML：能解析就同步回字段，解析不了保持原样不炸
  const handleTomlEdit = (value: string) => {
    lastTomlRef.current = value;
    onTomlChange(value);
    const parsed = parseKimiCodeFields(value);
    if (parsed) setFields(parsed);
  };

  // 解析出的模型不在预设列表时（如手改 TOML），临时加入下拉避免丢失
  const modelOptions = useMemo(() => {
    const options: string[] = [...KIMI_CODE_MODEL_OPTIONS];
    if (fields.model && !options.includes(fields.model)) {
      options.unshift(fields.model);
    }
    return options;
  }, [fields.model]);

  return (
    <div className="space-y-4">
      <ApiKeyInput
        id="kimi-code-api-key"
        label={t("kimiCode.form.apiKey", { defaultValue: "API Key" })}
        value={fields.apiKey}
        onChange={(value) => updateFields({ apiKey: value })}
        required
      />

      <div className="space-y-2">
        <Label htmlFor="kimi-code-base-url">
          {t("kimiCode.form.baseUrl", { defaultValue: "Base URL" })}
        </Label>
        <Input
          id="kimi-code-base-url"
          value={fields.baseUrl}
          onChange={(event) => updateFields({ baseUrl: event.target.value })}
          placeholder={KIMI_CODE_DEFAULT_BASE_URL}
          spellCheck={false}
        />
      </div>

      <div className="space-y-2">
        <Label>{t("kimiCode.form.model", { defaultValue: "模型" })}</Label>
        <Select
          value={fields.model}
          onValueChange={(value) => updateFields({ model: value })}
        >
          <SelectTrigger>
            <SelectValue
              placeholder={t("kimiCode.form.model", { defaultValue: "模型" })}
            />
          </SelectTrigger>
          <SelectContent>
            {modelOptions.map((model) => (
              <SelectItem key={model} value={model}>
                {model}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
        <CollapsibleTrigger asChild>
          <Button
            type="button"
            variant={null}
            size="sm"
            className="h-8 gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
          >
            {advancedOpen ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
            {t("kimiCode.form.advancedToml", {
              defaultValue: "高级配置（TOML）",
            })}
          </Button>
        </CollapsibleTrigger>
        <CollapsibleContent className="space-y-2 pt-2">
          <Textarea
            id="kimi-code-config"
            value={toml}
            onChange={(event) => handleTomlEdit(event.target.value)}
            placeholder={t("kimiCode.form.configPlaceholder", {
              defaultValue: 'default_model = "kimi-code/k3"',
            })}
            className="min-h-[240px] resize-y font-mono text-xs leading-5"
            spellCheck={false}
          />
          <p className="text-xs text-muted-foreground">
            {t("kimiCode.form.configHint", {
              defaultValue:
                "直接编辑完整 TOML，保存后写入 ~/.kimi-code/config.toml；上面的字段修改会同步到这里。",
            })}
          </p>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}
