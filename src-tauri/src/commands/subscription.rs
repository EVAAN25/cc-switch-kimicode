use std::str::FromStr;
use tauri::{Emitter, State};

use crate::app_config::AppType;
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;

/// 查询官方订阅额度
///
/// 读取 CLI 工具已有的 OAuth 凭据并调用官方 API 获取使用额度。
/// `Ok`（成功或确定性失败）写入 `UsageCache`、通知托盘刷新并 emit
/// `usage-cache-updated`，让前端 React Query 与托盘共享同一份最新数据；失败
/// 快照写入后 `format_subscription_summary` 会通过 `success=false` 守卫返回
/// `None`，托盘 suffix 自然消失，避免长期滞留旧配额数字。
/// `Err`（瞬时传输失败）不写快照、不 emit：保留上一份托盘快照，与前端
/// react-query reject 保留上次 data 的语义一致（emit 失败快照会经
/// `useUsageCacheBridge` 盲写回 query 缓存，抹掉本该保留的旧值）。
#[tauri::command]
pub async fn get_subscription_quota(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    tool: String,
) -> Result<SubscriptionQuota, String> {
    let inner = crate::services::subscription::get_subscription_quota(&tool).await;
    if let Ok(snapshot) = &inner {
        if let Ok(app_type) = AppType::from_str(&tool) {
            let payload = serde_json::json!({
                "kind": "subscription",
                "appType": app_type.as_str(),
                "data": snapshot,
            });
            if let Err(e) = app.emit("usage-cache-updated", payload) {
                log::error!("emit usage-cache-updated (subscription) 失败: {e}");
            }
            state
                .usage_cache
                .put_subscription(app_type, snapshot.clone());
            crate::tray::schedule_tray_refresh(&app);
        }
    }
    inner
}

/// 按 provider 条目查询 Kimi Code 用量（条目自身 api_key 优先，OAuth 兜底）。
///
/// 与 `get_subscription_quota` 同为 `Ok` 时写快照/刷新托盘的语义，但仅当该
/// 条目是当前生效供应商时才写——usage_cache 的 subscription 槽位按 appType
/// 只有一份，非当前条目的快照写进去会让托盘显示成别的账号的额度。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_kimi_code_provider_quota(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<SubscriptionQuota, String> {
    let inner =
        crate::services::subscription::get_kimi_code_provider_quota(&state, &provider_id).await;
    if let Ok(snapshot) = &inner {
        let is_current = state
            .db
            .get_current_provider(AppType::KimiCode.as_str())
            .map(|current| current.as_deref() == Some(provider_id.as_str()))
            .unwrap_or(false);
        if is_current {
            let payload = serde_json::json!({
                "kind": "subscription",
                "appType": AppType::KimiCode.as_str(),
                "data": snapshot,
            });
            if let Err(e) = app.emit("usage-cache-updated", payload) {
                log::error!("emit usage-cache-updated (kimi-code provider quota) 失败: {e}");
            }
            state
                .usage_cache
                .put_subscription(AppType::KimiCode, snapshot.clone());
            crate::tray::schedule_tray_refresh(&app);
        }
    }
    inner
}
