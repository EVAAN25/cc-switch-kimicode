import { parse as parseToml, stringify as stringifyToml } from "smol-toml";

/** Kimi Code 的默认 config.toml。表单在 Provider settingsConfig 中
 * 以 { config: string } 的 JSON 形式暂存，提交时由后端写回 TOML 文件。 */
export const KIMI_CODE_DEFAULT_BASE_URL = "https://api.kimi.com/coding/v1";
export const KIMI_CODE_DEFAULT_MODEL = "k3";
export const KIMI_CODE_MODEL_OPTIONS = [
  "k3",
  "kimi-for-coding",
  "kimi-for-coding-highspeed",
] as const;
export const KIMI_CODE_PROVIDER_KEY = "kimi-code";
export const KIMI_CODE_MAX_CONTEXT_SIZE = 262144;

export const KIMI_CODE_DEFAULT_TOML = `default_model = "kimi-code/k3"

[providers.kimi-code]
type = "kimi"
api_key = ""
base_url = "https://api.kimi.com/coding/v1"

[models."kimi-code/k3"]
provider = "kimi-code"
model = "k3"
max_context_size = 262144
`;

export const KIMI_CODE_DEFAULT_CONFIG = JSON.stringify(
  { config: KIMI_CODE_DEFAULT_TOML },
  null,
  2,
);

export function extractKimiCodeToml(settingsConfig: string): string {
  try {
    const parsed = JSON.parse(settingsConfig) as {
      config?: unknown;
    };
    return typeof parsed.config === "string" ? parsed.config : "";
  } catch {
    return "";
  }
}

export function serializeKimiCodeToml(config: string): string {
  return JSON.stringify({ config }, null, 2);
}

/** 结构化表单字段：API Key / Base URL / 模型。 */
export interface KimiCodeFormFieldsData {
  apiKey: string;
  baseUrl: string;
  model: string;
}

type TomlTable = Record<string, unknown>;

function asTable(value: unknown): TomlTable | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as TomlTable)
    : undefined;
}

/** 找到承载密钥/端点的 provider 段名：优先 default_model 前缀，
 * 否则取第一个带 api_key 或 type = "kimi" 的 provider。 */
function resolveProviderKey(doc: TomlTable): string {
  const providers = asTable(doc.providers);
  const defaultModel =
    typeof doc.default_model === "string" ? doc.default_model : "";
  const prefix = defaultModel.includes("/")
    ? defaultModel.slice(0, defaultModel.lastIndexOf("/"))
    : "";
  if (providers) {
    if (prefix && asTable(providers[prefix])) return prefix;
    for (const [key, value] of Object.entries(providers)) {
      const table = asTable(value);
      if (
        table &&
        (typeof table.api_key === "string" || table.type === "kimi")
      ) {
        return key;
      }
    }
  }
  return prefix || KIMI_CODE_PROVIDER_KEY;
}

/** 从 TOML 解析出结构化字段；TOML 为空或解析失败返回 null。 */
export function parseKimiCodeFields(
  toml: string,
): KimiCodeFormFieldsData | null {
  if (!toml.trim()) return null;
  try {
    const doc = parseToml(toml) as TomlTable;
    const providers = asTable(doc.providers);
    const providerKey = resolveProviderKey(doc);
    const provider = providers ? asTable(providers[providerKey]) : undefined;
    const defaultModel =
      typeof doc.default_model === "string" ? doc.default_model : "";
    const model = defaultModel.includes("/")
      ? defaultModel.slice(defaultModel.lastIndexOf("/") + 1)
      : defaultModel;
    return {
      apiKey: typeof provider?.api_key === "string" ? provider.api_key : "",
      baseUrl:
        typeof provider?.base_url === "string" && provider.base_url.trim()
          ? provider.base_url
          : KIMI_CODE_DEFAULT_BASE_URL,
      model: model.trim() || KIMI_CODE_DEFAULT_MODEL,
    };
  } catch {
    return null;
  }
}

/** 把结构化字段写回 TOML：只改 default_model、生效 provider 的
 * api_key/base_url 及对应 models 段，其余内容原样保留。
 * TOML 解析失败时基于默认模板重建。 */
export function applyKimiCodeFields(
  toml: string,
  fields: KimiCodeFormFieldsData,
): string {
  let doc: TomlTable;
  try {
    doc = parseToml(toml) as TomlTable;
  } catch {
    doc = parseToml(KIMI_CODE_DEFAULT_TOML) as TomlTable;
  }

  const providerKey = resolveProviderKey(doc);
  const providers = asTable(doc.providers) ?? {};
  doc.providers = providers;
  const provider = asTable(providers[providerKey]) ?? { type: "kimi" };
  providers[providerKey] = provider;

  provider.api_key = fields.apiKey;
  if (fields.apiKey.trim()) {
    // 填写 API Key 即切换为密钥鉴权，移除 oauth 段避免两种凭据并存
    delete provider.oauth;
  }
  provider.base_url = fields.baseUrl.trim() || KIMI_CODE_DEFAULT_BASE_URL;

  const model = fields.model.trim() || KIMI_CODE_DEFAULT_MODEL;
  const modelRef = `${providerKey}/${model}`;
  doc.default_model = modelRef;

  const models = asTable(doc.models) ?? {};
  doc.models = models;
  if (!asTable(models[modelRef])) {
    // 新模型段沿用已有模型段的上下文长度，缺省 262144
    let maxContextSize = KIMI_CODE_MAX_CONTEXT_SIZE;
    for (const value of Object.values(models)) {
      const table = asTable(value);
      if (typeof table?.max_context_size === "number") {
        maxContextSize = table.max_context_size;
        break;
      }
    }
    models[modelRef] = {
      provider: providerKey,
      model,
      max_context_size: maxContextSize,
    };
  }

  return stringifyToml(doc);
}
