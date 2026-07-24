#![allow(non_snake_case)]

//! Kimi Code 任务提醒（hooks 音效）管理。
//!
//! Kimi Code 支持在 config.toml 里配置 `[[hooks]]`：event 触发时执行 command。
//! 本模块把「播放提示音」封装为 cc-switch 管理的 hook——command 末尾带
//! `# ccswitch-notify` 标记，读写时按标记识别，完整保留用户自己的 hooks。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::AppError;
use crate::kimi_code_config;

/// 受管理 hook 的识别标记（command 末尾注释）。
const MANAGED_HOOK_MARKER: &str = "# ccswitch-notify";
/// Notification hook 用于「后台任务完成」事件的 matcher（TOML 字符串值，kimi-code 按正则解析）。
const TASK_COMPLETED_MATCHER: &str = "task\\.completed";
/// 资源目录不可读时的兜底音效列表（与 src-tauri/resources/sounds/ 保持一致）。
const DEFAULT_SOUND_NAMES: [&str; 5] = ["ding", "chime", "woodblock", "success", "gentle"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiNotifySettings {
    pub enabled: bool,
    pub stop_sound: Option<String>,
    pub task_completed_sound: Option<String>,
    pub subagent_stop_sound: Option<String>,
}

/// 打包进 app 的音效目录。
///
/// release 下走 tauri 资源解析（bundle.resources 把 `resources/sounds/`
/// 打进资源目录）；dev 下资源不会被复制，回退到编译期的 manifest 路径。
fn bundled_sounds_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let resolver = app.path();
    for candidate in ["sounds", "resources/sounds"] {
        if let Ok(path) = resolver.resolve(candidate, tauri::path::BaseDirectory::Resource) {
            if path.is_dir() {
                return Ok(path);
            }
        }
    }

    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("sounds");
    if dev_path.is_dir() {
        return Ok(dev_path);
    }

    Err("未找到应用内置音效目录（resources/sounds）".to_string())
}

/// 音效名只允许安全字符，防止路径穿越。
fn is_valid_sound_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 列出可用音效名（不含扩展名）。
#[tauri::command]
pub async fn list_kimi_notify_sounds(app: AppHandle) -> Result<Vec<String>, String> {
    let dir = bundled_sounds_dir(&app)?;
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let path = entry.path();
                    (path.extension().is_some_and(|ext| ext == "wav"))
                        .then(|| path.file_stem().map(|s| s.to_string_lossy().to_string()))
                        .flatten()
                })
                .collect()
        })
        .unwrap_or_default();
    if names.is_empty() {
        names = DEFAULT_SOUND_NAMES.iter().map(|s| s.to_string()).collect();
    }
    names.sort();
    Ok(names)
}

/// 生成 hook command：按宿主 OS 播放指定 wav 文件（纯函数，路径为字符串）。
fn notify_play_command(path: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        format!("afplay \"{path}\"")
    }
    #[cfg(target_os = "linux")]
    {
        format!("paplay \"{path}\" || canberra-gtk-play -f \"{path}\"")
    }
    #[cfg(target_os = "windows")]
    {
        format!(
            "powershell -NoProfile -Command \"(New-Object Media.SoundPlayer '{path}').PlaySync();\""
        )
    }
}

/// 试听：异步 spawn 系统播放器，不阻塞。
#[tauri::command]
pub async fn preview_kimi_notify_sound(app: AppHandle, name: String) -> Result<bool, String> {
    if !is_valid_sound_name(&name) {
        return Err(format!("无效的音效名: {name}"));
    }
    let path = bundled_sounds_dir(&app)?.join(format!("{name}.wav"));
    if !path.is_file() {
        return Err(format!("音效文件不存在: {}", path.display()));
    }
    spawn_sound_player(&path)?;
    Ok(true)
}

#[cfg(target_os = "macos")]
fn spawn_sound_player(path: &Path) -> Result<(), String> {
    std::process::Command::new("afplay")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动 afplay 失败: {e}"))
}

#[cfg(target_os = "linux")]
fn spawn_sound_player(path: &Path) -> Result<(), String> {
    // 优先 paplay（PulseAudio/PipeWire），spawn 失败回退 libcanberra。
    match std::process::Command::new("paplay").arg(path).spawn() {
        Ok(_) => Ok(()),
        Err(_) => std::process::Command::new("canberra-gtk-play")
            .arg("-f")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("启动 paplay / canberra-gtk-play 失败: {e}")),
    }
}

#[cfg(target_os = "windows")]
fn spawn_sound_player(path: &Path) -> Result<(), String> {
    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "(New-Object Media.SoundPlayer '{}').PlaySync();",
                path.to_string_lossy()
            ),
        ])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动 powershell 播放失败: {e}"))
}

/// 读取当前提醒设置。config.toml 不存在或解析失败时返回全空配置（不报错）。
#[tauri::command]
pub async fn get_kimi_notify_settings() -> Result<KimiNotifySettings, String> {
    let Ok(text) = std::fs::read_to_string(kimi_code_config::get_kimi_code_config_path()) else {
        return Ok(KimiNotifySettings::default());
    };
    Ok(parse_notify_settings_text(&text))
}

/// 保存提醒设置：复制音效到 `{kimi_code_home}/sounds/`，改写 config.toml 的受管理 hooks。
#[tauri::command]
pub async fn set_kimi_notify_settings(
    app: AppHandle,
    settings: KimiNotifySettings,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || set_kimi_notify_settings_blocking(&app, &settings))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(true)
}

fn set_kimi_notify_settings_blocking(
    app: &AppHandle,
    settings: &KimiNotifySettings,
) -> Result<(), AppError> {
    let config_path = kimi_code_config::get_kimi_code_config_path();
    if !config_path.exists() {
        return Err(AppError::localized(
            "kimi_code.notify.config.missing",
            "未找到 Kimi Code 配置文件，请先安装并登录 Kimi Code",
            "Kimi Code config.toml not found; please install and sign in to Kimi Code first",
        ));
    }

    // a. 把选中的 wav 复制到 {kimi_code_home}/sounds/
    let selected: Vec<&str> = [
        settings.stop_sound.as_deref(),
        settings.task_completed_sound.as_deref(),
        settings.subagent_stop_sound.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let sounds_dir = kimi_code_config::get_kimi_code_home().join("sounds");
    if settings.enabled && !selected.is_empty() {
        let bundled = bundled_sounds_dir(app).map_err(|e| {
            AppError::localized("kimi_code.notify.sounds.missing", e.clone(), e)
        })?;
        std::fs::create_dir_all(&sounds_dir).map_err(|e| AppError::io(&sounds_dir, e))?;
        for name in selected {
            if !is_valid_sound_name(name) {
                return Err(AppError::localized(
                    "kimi_code.notify.sound.invalid",
                    format!("无效的音效名: {name}"),
                    format!("Invalid sound name: {name}"),
                ));
            }
            let source = bundled.join(format!("{name}.wav"));
            if !source.is_file() {
                return Err(AppError::localized(
                    "kimi_code.notify.sound.missing",
                    format!("音效文件不存在: {name}.wav"),
                    format!("Sound file not found: {name}.wav"),
                ));
            }
            let target = sounds_dir.join(format!("{name}.wav"));
            std::fs::copy(&source, &target).map_err(|e| AppError::io(&target, e))?;
        }
    }

    // b/c. 读当前文本，删除受管理 hooks 后按选择追加
    let text = std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;
    let rendered = render_notify_hooks_text(&text, settings, &sounds_dir)?;

    // d. 校验后写入（不能用 write_kimi_code_config_text：它会把磁盘上刚删掉的
    //    受管理 hooks 再合并回来）
    kimi_code_config::validate_kimi_code_config_text(&rendered)?;
    crate::config::write_text_file(&config_path, &rendered)
}

// ---------------------------------------------------------------------------
// 纯函数：TOML 文本解析与改写（便于单元测试）
// ---------------------------------------------------------------------------

/// command 是否带受管理标记。
fn is_managed_hook(table: &toml_edit::Table) -> bool {
    table
        .get("command")
        .and_then(toml_edit::Item::as_str)
        .is_some_and(|command| command.contains(MANAGED_HOOK_MARKER))
}

/// 从 hook command 里提取音效名（`<path>/<name>.wav` 的 name 部分）。
fn extract_sound_name(command: &str) -> Option<String> {
    let idx = command.find(".wav")?;
    let name: String = command[..idx]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if name.is_empty() { None } else { Some(name) }
}

/// 解析 config.toml 文本中的受管理 hooks，映射为提醒设置（纯函数）。
fn parse_notify_settings_text(text: &str) -> KimiNotifySettings {
    let mut settings = KimiNotifySettings::default();
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return settings;
    };
    let Some(hooks) = doc
        .get("hooks")
        .and_then(toml_edit::Item::as_array_of_tables)
    else {
        return settings;
    };

    for table in hooks.iter() {
        if !is_managed_hook(table) {
            continue;
        }
        settings.enabled = true;
        let command = table
            .get("command")
            .and_then(toml_edit::Item::as_str)
            .unwrap_or_default();
        let sound = extract_sound_name(command);
        let matcher = table
            .get("matcher")
            .and_then(toml_edit::Item::as_str)
            .unwrap_or_default()
            .replace('\\', "");
        match table.get("event").and_then(toml_edit::Item::as_str) {
            Some("Stop") => settings.stop_sound = sound,
            Some("Notification") if matcher.contains("task.completed") => {
                settings.task_completed_sound = sound
            }
            Some("SubagentStop") => settings.subagent_stop_sound = sound,
            _ => {}
        }
    }
    settings
}

/// 构造一条受管理 hook 表。
fn notify_hook_table(event: &str, matcher: Option<&str>, command: String) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    table["event"] = toml_edit::value(event);
    if let Some(matcher) = matcher {
        table["matcher"] = toml_edit::value(matcher);
    }
    table["command"] = toml_edit::value(format!("{command} {MANAGED_HOOK_MARKER}"));
    table
}

/// 改写 config.toml 文本：删除所有受管理 hooks，enabled 时按选择追加（纯函数）。
///
/// 非受管理 hooks 与文件其余内容完整保留；音效为 None 的事件不生成 hook。
fn render_notify_hooks_text(
    text: &str,
    settings: &KimiNotifySettings,
    sounds_dir: &Path,
) -> Result<String, AppError> {
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| {
        AppError::localized(
            "kimi_code.config.invalid",
            format!("Kimi Code config.toml 格式无效: {e}"),
            format!("Invalid Kimi Code config.toml: {e}"),
        )
    })?;

    if let Some(hooks) = doc
        .get_mut("hooks")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
    {
        let managed: Vec<usize> = hooks
            .iter()
            .enumerate()
            .filter(|(_, table)| is_managed_hook(table))
            .map(|(idx, _)| idx)
            .collect();
        for idx in managed.into_iter().rev() {
            hooks.remove(idx);
        }
    }
    // 删空后移除整个 hooks key，避免留下空表
    if doc
        .get("hooks")
        .and_then(toml_edit::Item::as_array_of_tables)
        .is_some_and(|hooks| hooks.is_empty())
    {
        doc.remove("hooks");
    }

    if settings.enabled {
        let item = doc
            .entry("hooks")
            .or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));
        let hooks = item.as_array_of_tables_mut().ok_or_else(|| {
            AppError::localized(
                "kimi_code.notify.hooks.invalid",
                "config.toml 中的 hooks 字段不是表数组",
                "The hooks field in config.toml is not an array of tables",
            )
        })?;

        let play = |name: &str| {
            notify_play_command(
                &sounds_dir
                    .join(format!("{name}.wav"))
                    .to_string_lossy(),
            )
        };
        if let Some(name) = &settings.stop_sound {
            hooks.push(notify_hook_table("Stop", None, play(name)));
        }
        if let Some(name) = &settings.task_completed_sound {
            hooks.push(notify_hook_table(
                "Notification",
                Some(TASK_COMPLETED_MATCHER),
                play(name),
            ));
        }
        if let Some(name) = &settings.subagent_stop_sound {
            hooks.push(notify_hook_table("SubagentStop", None, play(name)));
        }
    }

    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sounds_dir() -> PathBuf {
        PathBuf::from("/home/user/.kimi-code/sounds")
    }

    const BASE_CONFIG: &str = r#"default_model = "kimi-code/k3"

[providers.kimi-code]
type = "kimi"
api_key = "sk-test"
"#;

    fn managed_hook(event: &str, matcher: Option<&str>, name: &str) -> String {
        let mut s = format!("event = \"{event}\"\n");
        if let Some(m) = matcher {
            s.push_str(&format!("matcher = \"{m}\"\n"));
        }
        s.push_str(&format!(
            "command = \"afplay \\\"/home/user/.kimi-code/sounds/{name}.wav\\\" {MANAGED_HOOK_MARKER}\"\n"
        ));
        format!("[[hooks]]\n{s}")
    }

    #[test]
    fn parse_reads_managed_hooks() {
        let text = format!(
            "{BASE_CONFIG}\n[[hooks]]\nevent = \"Stop\"\ncommand = \"/usr/bin/notify-send hi\"\n\n{}\n{}\n",
            managed_hook("Stop", None, "ding"),
            managed_hook("Notification", Some("task\\\\.completed"), "chime"),
        );
        let settings = parse_notify_settings_text(&text);
        assert!(settings.enabled);
        assert_eq!(settings.stop_sound.as_deref(), Some("ding"));
        assert_eq!(settings.task_completed_sound.as_deref(), Some("chime"));
        assert_eq!(settings.subagent_stop_sound, None);
    }

    #[test]
    fn parse_empty_config_returns_defaults() {
        let settings = parse_notify_settings_text(BASE_CONFIG);
        assert!(!settings.enabled);
        assert_eq!(settings.stop_sound, None);
    }

    #[test]
    fn render_removes_managed_keeps_unmanaged() {
        let text = format!(
            "{BASE_CONFIG}\n[[hooks]]\nevent = \"Stop\"\ncommand = \"/usr/bin/notify-send hi\"\n\n{}\n",
            managed_hook("Stop", None, "ding"),
        );
        let rendered =
            render_notify_hooks_text(&text, &KimiNotifySettings::default(), &sounds_dir()).unwrap();
        assert!(rendered.contains("notify-send"), "非受管理 hook 被误删: {rendered}");
        assert!(!rendered.contains(MANAGED_HOOK_MARKER));
        assert!(!rendered.contains("ding.wav"));
        assert!(rendered.contains("default_model"));
        assert!(rendered.contains("[providers.kimi-code]"));
    }

    #[test]
    fn render_appends_three_hooks_to_empty_config() {
        let settings = KimiNotifySettings {
            enabled: true,
            stop_sound: Some("ding".to_string()),
            task_completed_sound: Some("chime".to_string()),
            subagent_stop_sound: Some("success".to_string()),
        };
        let rendered = render_notify_hooks_text(BASE_CONFIG, &settings, &sounds_dir()).unwrap();
        assert_eq!(rendered.matches("[[hooks]]").count(), 3, "{rendered}");
        assert_eq!(rendered.matches(MANAGED_HOOK_MARKER).count(), 3);
        assert!(rendered.contains("event = \"Stop\""));
        assert!(rendered.contains("event = \"SubagentStop\""));
        // matcher 的 TOML 值应为 task\.completed（toml_edit 可能渲染为 literal string）
        let reparsed = rendered.parse::<toml_edit::DocumentMut>().unwrap();
        let matcher = reparsed
            .get("hooks")
            .and_then(toml_edit::Item::as_array_of_tables)
            .and_then(|hooks| {
                hooks.iter().find(|t| {
                    t.get("event").and_then(toml_edit::Item::as_str) == Some("Notification")
                })
            })
            .and_then(|t| t.get("matcher"))
            .and_then(toml_edit::Item::as_str);
        assert_eq!(matcher, Some("task\\.completed"));
        assert!(rendered.contains("ding.wav"));
        assert!(rendered.contains("chime.wav"));
        assert!(rendered.contains("success.wav"));
        // 往返解析
        let parsed = parse_notify_settings_text(&rendered);
        assert_eq!(parsed.stop_sound.as_deref(), Some("ding"));
        assert_eq!(parsed.task_completed_sound.as_deref(), Some("chime"));
        assert_eq!(parsed.subagent_stop_sound.as_deref(), Some("success"));
    }

    #[test]
    fn render_skips_events_without_sound() {
        let settings = KimiNotifySettings {
            enabled: true,
            stop_sound: None,
            task_completed_sound: Some("chime".to_string()),
            subagent_stop_sound: None,
        };
        let rendered = render_notify_hooks_text(BASE_CONFIG, &settings, &sounds_dir()).unwrap();
        assert_eq!(rendered.matches("[[hooks]]").count(), 1, "{rendered}");
        assert!(rendered.contains("Notification"));
        assert!(!rendered.contains("SubagentStop"));
    }

    #[test]
    fn render_disabled_leaves_no_hooks_key() {
        let rendered =
            render_notify_hooks_text(BASE_CONFIG, &KimiNotifySettings::default(), &sounds_dir())
                .unwrap();
        assert!(!rendered.contains("[[hooks]]"));
        assert!(!rendered.contains("hooks"));
    }

    #[test]
    fn render_is_idempotent() {
        let settings = KimiNotifySettings {
            enabled: true,
            stop_sound: Some("ding".to_string()),
            task_completed_sound: Some("chime".to_string()),
            subagent_stop_sound: Some("gentle".to_string()),
        };
        let once = render_notify_hooks_text(BASE_CONFIG, &settings, &sounds_dir()).unwrap();
        let twice = render_notify_hooks_text(&once, &settings, &sounds_dir()).unwrap();
        assert_eq!(twice.matches("[[hooks]]").count(), 3, "重复设置翻倍: {twice}");
        assert_eq!(twice.matches(MANAGED_HOOK_MARKER).count(), 3);
        assert_eq!(once, twice, "重复设置结果不一致");
    }

    #[test]
    fn extract_sound_name_handles_paths() {
        assert_eq!(
            extract_sound_name("afplay \"/Users/x/.kimi-code/sounds/ding.wav\" # ccswitch-notify"),
            Some("ding".to_string())
        );
        assert_eq!(
            extract_sound_name(
                "powershell -NoProfile -Command \"(New-Object Media.SoundPlayer 'C:\\Users\\x\\.kimi-code\\sounds\\wood-block.wav').PlaySync();\" # ccswitch-notify"
            ),
            Some("wood-block".to_string())
        );
        assert_eq!(extract_sound_name("notify-send hi"), None);
    }
}
