import { describe, expect, it } from "vitest";
import {
  KIMI_CODE_DEFAULT_BASE_URL,
  KIMI_CODE_DEFAULT_TOML,
  applyKimiCodeFields,
  parseKimiCodeFields,
} from "./kimiCode";

describe("kimiCode config helpers", () => {
  it("parses the default template", () => {
    expect(parseKimiCodeFields(KIMI_CODE_DEFAULT_TOML)).toEqual({
      apiKey: "",
      baseUrl: KIMI_CODE_DEFAULT_BASE_URL,
      model: "k3",
    });
  });

  it("returns null for empty or invalid TOML", () => {
    expect(parseKimiCodeFields("")).toBeNull();
    expect(parseKimiCodeFields("  ")).toBeNull();
    expect(parseKimiCodeFields("default_model = [oops")).toBeNull();
  });

  it("parses legacy managed/oauth configs", () => {
    const legacy = `default_model = "managed:kimi-code/kimi-for-coding"

[providers."managed:kimi-code"]
type = "kimi"
base_url = "https://example.com/v1"
oauth = { storage = "file", key = "oauth/kimi-code" }

[models."managed:kimi-code/kimi-for-coding"]
provider = "managed:kimi-code"
model = "kimi-for-coding"
max_context_size = 262144
`;
    expect(parseKimiCodeFields(legacy)).toEqual({
      apiKey: "",
      baseUrl: "https://example.com/v1",
      model: "kimi-for-coding",
    });
  });

  it("applies fields and keeps the rest of the document", () => {
    const next = applyKimiCodeFields(KIMI_CODE_DEFAULT_TOML, {
      apiKey: "sk-test",
      baseUrl: "https://proxy.example.com/v1",
      model: "kimi-for-coding-highspeed",
    });
    const parsed = parseKimiCodeFields(next);
    expect(parsed).toEqual({
      apiKey: "sk-test",
      baseUrl: "https://proxy.example.com/v1",
      model: "kimi-for-coding-highspeed",
    });
    // 新模型段被补齐，旧模型段保留
    expect(next).toContain('[models."kimi-code/kimi-for-coding-highspeed"]');
    expect(next).toContain('[models."kimi-code/k3"]');
  });

  it("removes oauth when an API key is set on a legacy config", () => {
    const legacy = `default_model = "managed:kimi-code/kimi-for-coding"

[providers."managed:kimi-code"]
type = "kimi"
base_url = "https://api.kimi.com/coding/v1"
oauth = { storage = "file", key = "oauth/kimi-code" }

[models."managed:kimi-code/kimi-for-coding"]
provider = "managed:kimi-code"
model = "kimi-for-coding"
max_context_size = 262144
`;
    const next = applyKimiCodeFields(legacy, {
      apiKey: "sk-test",
      baseUrl: "https://api.kimi.com/coding/v1",
      model: "kimi-for-coding",
    });
    expect(next).not.toContain("oauth");
    // provider 段名沿用原有的 managed:kimi-code
    expect(next).toContain('default_model = "managed:kimi-code/kimi-for-coding"');
    expect(parseKimiCodeFields(next)?.apiKey).toBe("sk-test");
  });

  it("falls back to the default template when current TOML is broken", () => {
    const next = applyKimiCodeFields("not = [valid", {
      apiKey: "sk-test",
      baseUrl: "",
      model: "k3",
    });
    expect(parseKimiCodeFields(next)).toEqual({
      apiKey: "sk-test",
      baseUrl: KIMI_CODE_DEFAULT_BASE_URL,
      model: "k3",
    });
  });
});
