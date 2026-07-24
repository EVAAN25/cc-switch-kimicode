//! Kimi Code 本地配置文件访问。
//!
//! Kimi Code 自己负责 OAuth 登录和凭据保存；cc-switch 只管理当前生效的
//! `config.toml`，并从同一目录读取用量查询所需的 OAuth 凭据。

use std::path::PathBuf;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::config;
use crate::error::AppError;

pub const DEFAULT_KIMI_CODE_HOME_NAME: &str = ".kimi-code";
pub const KIMI_CODE_CONFIG_FILE: &str = "config.toml";
pub const KIMI_CODE_DEFAULT_BASE_URL: &str = "https://api.kimi.com/coding/v1";
pub const KIMI_CODE_DEFAULT_OAUTH_HOST: &str = "https://auth.kimi.com";
pub const KIMI_CODE_DEFAULT_MODEL: &str = "kimi-for-coding";
pub const KIMI_CODE_MANAGED_PROVIDER: &str = "managed:kimi-code";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KimiCodeRuntimeAuth {
    pub(crate) base_url: String,
    pub(crate) oauth_host: String,
    pub(crate) credential_name: String,
}

/// Kimi Code 的主目录。
///
/// Kimi Code 官方支持 `KIMI_CODE_HOME`，优先使用它以兼容用户自定义安装。
pub fn get_kimi_code_home() -> PathBuf {
    std::env::var_os("KIMI_CODE_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| config::get_home_dir().join(DEFAULT_KIMI_CODE_HOME_NAME))
}

pub fn get_kimi_code_config_path() -> PathBuf {
    get_kimi_code_home().join(KIMI_CODE_CONFIG_FILE)
}

pub fn get_kimi_code_credentials_path() -> PathBuf {
    get_kimi_code_home().join("credentials").join(format!(
        "{}.json",
        get_kimi_code_runtime_auth().credential_name
    ))
}

fn normalize_endpoint(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn configured_managed_provider_values() -> (Option<String>, Option<String>, Option<String>) {
    let path = get_kimi_code_config_path();
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, None, None);
    };
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
        return (None, None, None);
    };

    let provider = document
        .get("providers")
        .and_then(|item| item.as_table_like())
        .and_then(|providers| providers.get(KIMI_CODE_MANAGED_PROVIDER))
        .and_then(|item| item.as_table_like());

    let Some(provider) = provider else {
        return (None, None, None);
    };

    let base_url = provider
        .get("base_url")
        .or_else(|| provider.get("baseUrl"))
        .and_then(|item| item.as_str())
        .map(str::to_string);
    let oauth = provider.get("oauth").and_then(|item| item.as_table_like());
    let oauth_key = oauth
        .and_then(|table| table.get("key"))
        .and_then(|item| item.as_str())
        .map(str::to_string);
    let oauth_host = oauth
        .and_then(|table| table.get("oauth_host").or_else(|| table.get("oauthHost")))
        .and_then(|item| item.as_str())
        .map(str::to_string);

    (base_url, oauth_key, oauth_host)
}

fn expected_oauth_key(base_url: &str, oauth_host: &str) -> String {
    if base_url == KIMI_CODE_DEFAULT_BASE_URL && oauth_host == KIMI_CODE_DEFAULT_OAUTH_HOST {
        return "oauth/kimi-code".to_string();
    }

    // Keep this byte-for-byte compatible with Kimi Code's
    // resolveKimiCodeOAuthKey(): JSON property order is oauthHost, baseUrl.
    let input = serde_json::to_string(&json!({
        "oauthHost": oauth_host,
        "baseUrl": base_url,
    }))
    .unwrap_or_default();
    let digest = Sha256::digest(input.as_bytes());
    let hex = format!("{digest:x}");
    format!("oauth/kimi-code-env-{}", &hex[..16])
}

fn credential_name_from_oauth_key(key: &str) -> String {
    key.rsplit('/')
        .next()
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("kimi-code")
        .to_string()
}

/// Resolve the same managed-Kimi runtime endpoints and file credential slot
/// used by Kimi Code itself. Environment overrides take precedence over the
/// provider block in config.toml.
pub(crate) fn get_kimi_code_runtime_auth() -> KimiCodeRuntimeAuth {
    let (configured_base_url, configured_key, configured_oauth_host) =
        configured_managed_provider_values();
    let env_base_url = non_empty_env("KIMI_CODE_BASE_URL");
    let env_oauth_host =
        non_empty_env("KIMI_CODE_OAUTH_HOST").or_else(|| non_empty_env("KIMI_OAUTH_HOST"));

    let base_url = normalize_endpoint(
        env_base_url
            .as_deref()
            .or(configured_base_url.as_deref())
            .unwrap_or(KIMI_CODE_DEFAULT_BASE_URL),
    );
    let oauth_host = normalize_endpoint(
        env_oauth_host
            .as_deref()
            .or(configured_oauth_host.as_deref())
            .unwrap_or(KIMI_CODE_DEFAULT_OAUTH_HOST),
    );
    let expected_key = expected_oauth_key(&base_url, &oauth_host);
    let has_environment_override = env_base_url.is_some() || env_oauth_host.is_some();
    let oauth_key =
        if !has_environment_override && configured_key.as_deref() == Some(expected_key.as_str()) {
            configured_key.unwrap_or(expected_key)
        } else {
            expected_key
        };

    KimiCodeRuntimeAuth {
        base_url,
        oauth_host,
        credential_name: credential_name_from_oauth_key(&oauth_key),
    }
}

pub fn read_kimi_code_config_text() -> Result<String, AppError> {
    let path = get_kimi_code_config_path();
    if !path.exists() {
        return Err(AppError::localized(
            "kimi_code.config.missing",
            "Kimi Code 配置文件不存在",
            "Kimi Code configuration file is missing",
        ));
    }

    std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
}

pub fn validate_kimi_code_config_text(text: &str) -> Result<(), AppError> {
    text.parse::<toml_edit::DocumentMut>().map_err(|e| {
        AppError::localized(
            "kimi_code.config.invalid",
            format!("Kimi Code config.toml 格式无效: {e}"),
            format!("Invalid Kimi Code config.toml: {e}"),
        )
    })?;
    Ok(())
}

pub fn write_kimi_code_config_text(text: &str) -> Result<(), AppError> {
    let merged = merge_existing_hooks(text);
    validate_kimi_code_config_text(&merged)?;
    config::write_text_file(&get_kimi_code_config_path(), &merged)
}

/// 从 hooks 表里提取去重键（event/matcher/command/timeout 四元组）。
fn hook_key(table: &toml_edit::Table) -> String {
    let get = |name: &str| {
        table
            .get(name)
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let timeout = table
        .get("timeout")
        .and_then(|item| item.as_integer())
        .map(|v| v.to_string())
        .unwrap_or_default();
    format!(
        "{}|{}|{}|{}",
        get("event"),
        get("matcher"),
        get("command"),
        timeout
    )
}

/// 把现有配置里的 [[hooks]] 合并进待写入文本（纯函数，便于测试）。
///
/// hooks 是用户级配置（桌面通知、审计等），与供应商无关。cc-switch 切换
/// 供应商时用条目保存的模板整体覆盖 config.toml，会把用户后来手动加的
/// [[hooks]] 冲掉，所以写入前把磁盘上现有的 hooks 补回来。模板里已有的
/// 同名条目（event/matcher/command/timeout 一致）不重复添加。
fn merge_hooks_text(existing_text: &str, new_text: &str) -> String {
    let Ok(existing_doc) = existing_text.parse::<toml_edit::DocumentMut>() else {
        return new_text.to_string();
    };
    let Ok(mut new_doc) = new_text.parse::<toml_edit::DocumentMut>() else {
        return new_text.to_string();
    };

    let Some(existing_hooks) = existing_doc
        .get("hooks")
        .and_then(|item| item.as_array_of_tables())
    else {
        return new_text.to_string();
    };
    if existing_hooks.is_empty() {
        return new_text.to_string();
    }

    let mut seen: std::collections::HashSet<String> = new_doc
        .get("hooks")
        .and_then(|item| item.as_array_of_tables())
        .map(|tables| tables.iter().map(hook_key).collect())
        .unwrap_or_default();

    let new_hooks_item = new_doc
        .entry("hooks")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));
    let Some(new_hooks) = new_hooks_item.as_array_of_tables_mut() else {
        return new_text.to_string();
    };

    let mut changed = false;
    for table in existing_hooks.iter() {
        if seen.insert(hook_key(table)) {
            new_hooks.push(table.clone());
            changed = true;
        }
    }

    if changed {
        new_doc.to_string()
    } else {
        new_text.to_string()
    }
}

/// 读取当前磁盘上的 config.toml，将其中的 [[hooks]] 合并进待写入文本。
fn merge_existing_hooks(new_text: &str) -> String {
    let path = get_kimi_code_config_path();
    let Ok(existing_text) = std::fs::read_to_string(&path) else {
        return new_text.to_string();
    };
    merge_hooks_text(&existing_text, new_text)
}

pub fn provider_config_text(settings: &Value) -> Result<&str, AppError> {
    settings
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::localized(
                "provider.kimi_code.config.missing",
                "Kimi Code 供应商缺少 config.toml 内容",
                "Kimi Code provider is missing config.toml content",
            )
        })
}

/// 尝试从 Kimi Code TOML 中提取 API key/base URL，供通用用量脚本兼容。
pub fn extract_credentials(settings: &Value) -> (String, String) {
    let Ok(text) = provider_config_text(settings) else {
        return (String::new(), String::new());
    };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return (String::new(), String::new());
    };

    let providers = doc
        .get("providers")
        .and_then(toml_edit::Item::as_table_like);
    let Some(providers) = providers else {
        return (String::new(), String::new());
    };

    for (_, item) in providers.iter() {
        let Some(table) = item.as_table_like() else {
            continue;
        };
        let base_url = table
            .get("base_url")
            .and_then(toml_edit::Item::as_str)
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_string();
        let api_key = table
            .get("api_key")
            .or_else(|| table.get("apiKey"))
            .and_then(toml_edit::Item::as_str)
            .unwrap_or_default()
            .to_string();
        if !api_key.is_empty() || !base_url.is_empty() {
            return (base_url, api_key);
        }
    }

    (String::new(), String::new())
}

#[cfg(test)]
mod tests {
    use super::merge_hooks_text;

    const PROVIDER_TEMPLATE: &str = r#"default_model = "kimi-code/k3"

[providers.kimi-code]
type = "kimi"
api_key = "sk-test"
base_url = "https://api.kimi.com/coding/v1"
"#;

    #[test]
    fn hooks_survive_provider_template_overwrite() {
        let existing = format!(
            "{PROVIDER_TEMPLATE}\n[[hooks]]\nevent = \"Stop\"\ncommand = \"/opt/homebrew/bin/terminal-notifier -title Kimi\"\n"
        );
        let merged = merge_hooks_text(&existing, PROVIDER_TEMPLATE);
        assert!(merged.contains("[[hooks]]"), "hooks 被覆盖丢失: {merged}");
        assert!(merged.contains("terminal-notifier"));
        // 模板原有内容不受影响
        assert!(merged.contains("default_model"));
        assert!(merged.contains("[providers.kimi-code]"));
    }

    #[test]
    fn hooks_deduped_when_template_already_has_them() {
        let hook = "[[hooks]]\nevent = \"Stop\"\ncommand = \"notify.sh\"\n";
        let existing = format!("{PROVIDER_TEMPLATE}\n{hook}");
        let new_text = format!("{PROVIDER_TEMPLATE}\n{hook}");
        let merged = merge_hooks_text(&existing, &new_text);
        assert_eq!(
            merged.matches("[[hooks]]").count(),
            1,
            "重复合并了 hooks: {merged}"
        );
    }

    #[test]
    fn distinct_hooks_both_kept() {
        let existing = format!(
            "{PROVIDER_TEMPLATE}\n[[hooks]]\nevent = \"Stop\"\ncommand = \"a.sh\"\n"
        );
        let new_text = format!(
            "{PROVIDER_TEMPLATE}\n[[hooks]]\nevent = \"SubagentStop\"\ncommand = \"b.sh\"\n"
        );
        let merged = merge_hooks_text(&existing, &new_text);
        assert!(merged.contains("a.sh"));
        assert!(merged.contains("b.sh"));
    }

    #[test]
    fn no_existing_hooks_returns_new_text() {
        let merged = merge_hooks_text(PROVIDER_TEMPLATE, PROVIDER_TEMPLATE);
        assert!(!merged.contains("[[hooks]]"));
    }

    #[test]
    fn broken_existing_toml_falls_back_to_new_text() {
        let merged = merge_hooks_text("not [ valid toml", PROVIDER_TEMPLATE);
        assert!(merged.contains("default_model"));
        assert!(!merged.contains("[[hooks]]"));
    }
}
