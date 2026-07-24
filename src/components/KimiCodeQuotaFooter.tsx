import React from "react";
import type { Provider } from "@/types";
import { useKimiCodeProviderQuota } from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import { parseKimiCodeFields } from "@/config/kimiCode";

interface KimiCodeQuotaFooterProps {
  provider: Provider;
  inline?: boolean;
  /** 是否为当前激活的供应商 */
  isCurrent?: boolean;
}

/**
 * Kimi Code 按条目的订阅额度 footer
 *
 * 复用 SubscriptionQuotaView 的全部渲染逻辑（5 状态 × inline/expanded）。
 * 数据源为后端 `get_kimi_code_provider_quota`：条目自己 TOML 配置里的
 * api_key/base_url 调 `/usages`，没配 key 时回退本机 OAuth 登录态。
 */
const KimiCodeQuotaFooter: React.FC<KimiCodeQuotaFooterProps> = ({
  provider,
  inline = false,
  isCurrent = false,
}) => {
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useKimiCodeProviderQuota(provider.id, {
    enabled: true,
    autoQuery: isCurrent,
  });

  // 与后端路由一致地判断该条目是否走 API key 鉴权，用于选择过期提示文案。
  const toml = (provider.settingsConfig as { config?: unknown })?.config;
  const hasApiKey = Boolean(
    typeof toml === "string" && parseKimiCodeFields(toml)?.apiKey.trim(),
  );

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      appIdForExpiredHint="kimi-code"
      expiredHintKey={
        hasApiKey
          ? "subscription.expiredHintApiKey"
          : "subscription.expiredHint"
      }
      inline={inline}
    />
  );
};

export default KimiCodeQuotaFooter;
