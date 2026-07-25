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
/// 系统音效设置值前缀（如 "system:Ping"），与内置 wav 名区分。
const SYSTEM_SOUND_PREFIX: &str = "system:";
/// macOS 系统音效目录（/System/Library/Sounds/<Name>.aiff）。
const SYSTEM_SOUNDS_DIR: &str = "/System/Library/Sounds";
/// macOS 系统音效白名单（仅 macOS 可用；白名单同时用于防路径注入）。
const SYSTEM_SOUND_NAMES: [&str; 14] = [
    "Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop", "Purr",
    "Sosumi", "Submarine", "Tink",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiNotifySettings {
    pub enabled: bool,
    pub stop_sound: Option<String>,
    pub task_completed_sound: Option<String>,
    pub subagent_stop_sound: Option<String>,
}

/// 可用音效列表，按来源分组。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiNotifySounds {
    /// 内置 wav（src-tauri/resources/sounds/）。
    pub bundled: Vec<String>,
    /// macOS 系统音效名（不含 "system:" 前缀；仅 macOS 非空）。
    pub system: Vec<String>,
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

/// 解析 "system:<Name>" 设置值；Name 必须在系统音效白名单内（防路径注入）。
fn parse_system_sound(value: &str) -> Option<&str> {
    let name = value.strip_prefix(SYSTEM_SOUND_PREFIX)?;
    SYSTEM_SOUND_NAMES.contains(&name).then_some(name)
}

/// 校验完整设置值（内置名或白名单内的 system: 名）。
fn is_valid_sound_value(value: &str) -> bool {
    parse_system_sound(value).is_some() || is_valid_sound_name(value)
}

/// 系统音效名列表：仅 macOS 非空。
#[cfg(target_os = "macos")]
fn system_sound_names() -> Vec<String> {
    SYSTEM_SOUND_NAMES.iter().map(|s| s.to_string()).collect()
}

#[cfg(not(target_os = "macos"))]
fn system_sound_names() -> Vec<String> {
    Vec::new()
}

/// 列出可用音效名（不含扩展名），按内置/系统分组。
#[tauri::command]
pub async fn list_kimi_notify_sounds(app: AppHandle) -> Result<KimiNotifySounds, String> {
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
    Ok(KimiNotifySounds {
        bundled: names,
        system: system_sound_names(),
    })
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

/// 事件对应的通知文案（terminal-notifier -message）。
fn event_message(event: &str) -> &'static str {
    match event {
        "Stop" => "可以回来看结果了",
        "Notification" => "后台任务完成",
        "SubagentStop" => "子任务完成",
        _ => "任务提醒",
    }
}

/// 系统音效 hook command（macOS）：terminal-notifier 横幅+声音优先，纯 afplay 兜底。
#[cfg(target_os = "macos")]
fn system_notify_command(name: &str, message: &str) -> Option<String> {
    Some(format!(
        "TN=$(command -v terminal-notifier || echo /opt/homebrew/bin/terminal-notifier); [ -x \"$TN\" ] && \"$TN\" -title \"Kimi Code\" -message \"{message}\" -sound \"{name}\" || afplay \"{SYSTEM_SOUNDS_DIR}/{name}.aiff\""
    ))
}

/// 非 macOS 平台不生成系统音效命令。
#[cfg(not(target_os = "macos"))]
fn system_notify_command(name: &str, message: &str) -> Option<String> {
    let _ = (name, message);
    None
}

/// 按设置值生成 hook command；无效值（如 system: 白名单外、非法字符）返回 None 跳过。
fn notify_hook_command(sound: &str, event: &str, sounds_dir: &Path) -> Option<String> {
    if let Some(system_name) = parse_system_sound(sound) {
        return system_notify_command(system_name, event_message(event));
    }
    if sound.starts_with(SYSTEM_SOUND_PREFIX) || !is_valid_sound_name(sound) {
        return None;
    }
    Some(notify_play_command(
        &sounds_dir.join(format!("{sound}.wav")).to_string_lossy(),
    ))
}

/// 试听：异步 spawn 系统播放器，不阻塞。
#[tauri::command]
pub async fn preview_kimi_notify_sound(app: AppHandle, name: String) -> Result<bool, String> {
    if let Some(system_name) = parse_system_sound(&name) {
        return preview_system_sound(system_name);
    }
    if name.starts_with(SYSTEM_SOUND_PREFIX) || !is_valid_sound_name(&name) {
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
fn preview_system_sound(name: &str) -> Result<bool, String> {
    let path = PathBuf::from(format!("{SYSTEM_SOUNDS_DIR}/{name}.aiff"));
    if !path.is_file() {
        return Err(format!("系统音效文件不存在: {}", path.display()));
    }
    spawn_sound_player(&path)?;
    Ok(true)
}

#[cfg(not(target_os = "macos"))]
fn preview_system_sound(name: &str) -> Result<bool, String> {
    let _ = name;
    Err("系统音效仅支持 macOS".to_string())
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

    // a. 把选中的内置 wav 复制到 {kimi_code_home}/sounds/（系统音效无需复制）
    let selected: Vec<&str> = [
        settings.stop_sound.as_deref(),
        settings.task_completed_sound.as_deref(),
        settings.subagent_stop_sound.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    for name in &selected {
        if !is_valid_sound_value(name) {
            return Err(AppError::localized(
                "kimi_code.notify.sound.invalid",
                format!("无效的音效名: {name}"),
                format!("Invalid sound name: {name}"),
            ));
        }
    }
    let bundled_selected: Vec<&str> = selected
        .iter()
        .filter(|name| parse_system_sound(name).is_none())
        .copied()
        .collect();
    let sounds_dir = kimi_code_config::get_kimi_code_home().join("sounds");
    if settings.enabled && !bundled_selected.is_empty() {
        let bundled = bundled_sounds_dir(app).map_err(|e| {
            AppError::localized("kimi_code.notify.sounds.missing", e.clone(), e)
        })?;
        std::fs::create_dir_all(&sounds_dir).map_err(|e| AppError::io(&sounds_dir, e))?;
        for name in bundled_selected {
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

/// 从 hook command 里提取内置音效名（`<path>/<name>.wav` 的 name 部分）。
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

/// 从 command 里提取 macOS 系统音效名（/System/Library/Sounds/<Name>.aiff）。
fn extract_system_aiff_name(command: &str) -> Option<String> {
    let start = command.find(SYSTEM_SOUNDS_DIR)? + SYSTEM_SOUNDS_DIR.len();
    let rest = command[start..].trim_start_matches('/');
    let end = rest.find(".aiff")?;
    let name = &rest[..end];
    is_valid_sound_name(name).then(|| name.to_string())
}

/// 从 terminal-notifier command 里提取 `-sound <Name>` 的值（兼容引号）。
fn extract_tn_sound_arg(command: &str) -> Option<String> {
    let idx = command.find("-sound")?;
    let rest = command[idx + "-sound".len()..].trim_start();
    let name = if let Some(quoted) = rest.strip_prefix('"') {
        quoted.split('"').next()?
    } else if let Some(quoted) = rest.strip_prefix('\'') {
        quoted.split('\'').next()?
    } else {
        rest.split_whitespace().next()?
    };
    is_valid_sound_name(name).then(|| name.to_string())
}

/// 从受管理 hook 的 command 提取完整设置值（system:<Name> 或内置名）。
fn extract_sound_value(command: &str) -> Option<String> {
    if let Some(name) = extract_system_aiff_name(command) {
        return Some(format!("{SYSTEM_SOUND_PREFIX}{name}"));
    }
    if command.contains("terminal-notifier") {
        if let Some(name) = extract_tn_sound_arg(command) {
            return Some(format!("{SYSTEM_SOUND_PREFIX}{name}"));
        }
    }
    extract_sound_name(command)
}

/// 识别「遗产 hook」（用户手动配置的音效通知，无受管理标记）：
/// terminal-notifier 且能提取 -sound，或引用 /System/Library/Sounds 的播放命令。
/// 判定刻意收窄，避免误删用户的其他自定义 hooks（如跑脚本的）。
fn legacy_notify_sound(command: &str) -> Option<String> {
    if command.contains(MANAGED_HOOK_MARKER) {
        return None;
    }
    if let Some(name) = extract_system_aiff_name(command) {
        return Some(format!("{SYSTEM_SOUND_PREFIX}{name}"));
    }
    if command.contains("terminal-notifier") {
        return extract_tn_sound_arg(command)
            .map(|name| format!("{SYSTEM_SOUND_PREFIX}{name}"));
    }
    None
}

/// 受管理 hook 或遗产 hook（保存时都会被新格式重写）。
fn is_notify_hook(table: &toml_edit::Table) -> bool {
    let Some(command) = table.get("command").and_then(toml_edit::Item::as_str) else {
        return false;
    };
    command.contains(MANAGED_HOOK_MARKER) || legacy_notify_sound(command).is_some()
}

/// 解析 config.toml 文本中的受管理/遗产 hooks，映射为提醒设置（纯函数）。
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
        let command = table
            .get("command")
            .and_then(toml_edit::Item::as_str)
            .unwrap_or_default();
        let sound = if command.contains(MANAGED_HOOK_MARKER) {
            settings.enabled = true;
            extract_sound_value(command)
        } else {
            let legacy = legacy_notify_sound(command);
            if legacy.is_some() {
                settings.enabled = true;
            }
            legacy
        };
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

/// 改写 config.toml 文本：删除所有受管理/遗产 hooks，enabled 时按选择追加（纯函数）。
///
/// 非音效类自定义 hooks 与文件其余内容完整保留；音效为 None 的事件不生成 hook。
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
            .filter(|(_, table)| is_notify_hook(table))
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

        if let Some(name) = &settings.stop_sound {
            if let Some(command) = notify_hook_command(name, "Stop", sounds_dir) {
                hooks.push(notify_hook_table("Stop", None, command));
            }
        }
        if let Some(name) = &settings.task_completed_sound {
            if let Some(command) = notify_hook_command(name, "Notification", sounds_dir) {
                hooks.push(notify_hook_table(
                    "Notification",
                    Some(TASK_COMPLETED_MATCHER),
                    command,
                ));
            }
        }
        if let Some(name) = &settings.subagent_stop_sound {
            if let Some(command) = notify_hook_command(name, "SubagentStop", sounds_dir) {
                hooks.push(notify_hook_table("SubagentStop", None, command));
            }
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

    // -----------------------------------------------------------------------
    // system: 音效与遗产 hook
    // -----------------------------------------------------------------------

    #[test]
    fn parse_recognizes_legacy_terminal_notifier_hooks() {
        let text = format!(
            "{BASE_CONFIG}\n[[hooks]]\nevent = \"SubagentStop\"\ncommand = \"/opt/homebrew/bin/terminal-notifier -title \\\"Kimi Code\\\" -message \\\"子任务完成\\\" -sound Purr\"\n\n[[hooks]]\nevent = \"Stop\"\ncommand = \"terminal-notifier -title 'Kimi Code' -sound \\\"Ping\\\"\"\n"
        );
        let settings = parse_notify_settings_text(&text);
        assert!(settings.enabled);
        assert_eq!(settings.subagent_stop_sound.as_deref(), Some("system:Purr"));
        // 引号包裹的 -sound 值也能提取
        assert_eq!(settings.stop_sound.as_deref(), Some("system:Ping"));
    }

    #[test]
    fn parse_recognizes_legacy_afplay_system_sound() {
        let text = format!(
            "{BASE_CONFIG}\n[[hooks]]\nevent = \"Stop\"\ncommand = \"afplay /System/Library/Sounds/Glass.aiff\"\n"
        );
        let settings = parse_notify_settings_text(&text);
        assert!(settings.enabled);
        assert_eq!(settings.stop_sound.as_deref(), Some("system:Glass"));
    }

    #[test]
    fn parse_ignores_non_sound_custom_hooks() {
        // 跑脚本的自定义 hook、无 -sound 的 terminal-notifier 都不识别
        let text = format!(
            "{BASE_CONFIG}\n[[hooks]]\nevent = \"Stop\"\ncommand = \"node ~/x.js\"\n\n[[hooks]]\nevent = \"Stop\"\ncommand = \"terminal-notifier -title 'Kimi Code' -message 'hi'\"\n"
        );
        let settings = parse_notify_settings_text(&text);
        assert!(!settings.enabled);
        assert_eq!(settings.stop_sound, None);
    }

    #[test]
    fn render_replaces_legacy_hook_with_single_managed_copy() {
        let text = format!(
            "{BASE_CONFIG}\n[[hooks]]\nevent = \"SubagentStop\"\ncommand = \"/opt/homebrew/bin/terminal-notifier -title \\\"Kimi Code\\\" -message \\\"旧消息\\\" -sound Purr\"\n"
        );
        let settings = KimiNotifySettings {
            enabled: true,
            stop_sound: None,
            task_completed_sound: None,
            subagent_stop_sound: Some("system:Purr".to_string()),
        };
        let rendered = render_notify_hooks_text(&text, &settings, &sounds_dir()).unwrap();
        assert_eq!(rendered.matches("[[hooks]]").count(), 1, "遗产 hook 未被接管: {rendered}");
        assert_eq!(rendered.matches(MANAGED_HOOK_MARKER).count(), 1);
        assert!(!rendered.contains("旧消息"), "旧 hook 未被删除: {rendered}");
        // 往返解析：遗产 hook 被新格式重写后仍识别为 system:Purr
        let parsed = parse_notify_settings_text(&rendered);
        assert_eq!(parsed.subagent_stop_sound.as_deref(), Some("system:Purr"));
    }

    #[test]
    fn render_keeps_custom_script_hooks() {
        let text = format!(
            "{BASE_CONFIG}\n[[hooks]]\nevent = \"Stop\"\ncommand = \"node ~/x.js\"\n\n[[hooks]]\nevent = \"Stop\"\ncommand = \"afplay /System/Library/Sounds/Glass.aiff\"\n"
        );
        let settings = KimiNotifySettings {
            enabled: true,
            stop_sound: Some("ding".to_string()),
            task_completed_sound: None,
            subagent_stop_sound: None,
        };
        let rendered = render_notify_hooks_text(&text, &settings, &sounds_dir()).unwrap();
        assert!(rendered.contains("node ~/x.js"), "自定义脚本 hook 被误删: {rendered}");
        // 遗产 afplay hook 被替换为受管理格式
        assert!(!rendered.contains("/System/Library/Sounds/Glass.aiff"));
        assert_eq!(rendered.matches(MANAGED_HOOK_MARKER).count(), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_sound_command_prefers_banner_with_sound_fallback() {
        let command = notify_hook_command("system:Ping", "Stop", &sounds_dir()).unwrap();
        assert!(command.contains("command -v terminal-notifier"), "{command}");
        assert!(command.contains("-sound \"Ping\""), "{command}");
        assert!(command.contains("-message \"可以回来看结果了\""), "{command}");
        // 纯声音兜底分支
        assert!(
            command.contains("|| afplay \"/System/Library/Sounds/Ping.aiff\""),
            "{command}"
        );
        // 事件文案映射
        let sub = notify_hook_command("system:Pop", "SubagentStop", &sounds_dir()).unwrap();
        assert!(sub.contains("-message \"子任务完成\""), "{sub}");
        let notif = notify_hook_command("system:Pop", "Notification", &sounds_dir()).unwrap();
        assert!(notif.contains("-message \"后台任务完成\""), "{notif}");
    }

    #[test]
    fn invalid_sound_values_are_rejected() {
        // 白名单内
        assert_eq!(parse_system_sound("system:Ping"), Some("Ping"));
        assert!(is_valid_sound_value("system:Ping"));
        assert!(is_valid_sound_value("ding"));
        // 路径注入 / 白名单外一律拒绝
        assert_eq!(parse_system_sound("system:../etc"), None);
        assert!(!is_valid_sound_value("system:../etc"));
        assert!(!is_valid_sound_value("system:NotASound"));
        assert!(!is_valid_sound_value("../ding"));
        // 非法值不生成 hook command
        assert!(notify_hook_command("system:../etc", "Stop", &sounds_dir()).is_none());
        assert!(notify_hook_command("../ding", "Stop", &sounds_dir()).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn managed_system_hook_roundtrips() {
        let settings = KimiNotifySettings {
            enabled: true,
            stop_sound: Some("system:Ping".to_string()),
            task_completed_sound: None,
            subagent_stop_sound: None,
        };
        let rendered = render_notify_hooks_text(BASE_CONFIG, &settings, &sounds_dir()).unwrap();
        assert_eq!(rendered.matches("[[hooks]]").count(), 1, "{rendered}");
        let parsed = parse_notify_settings_text(&rendered);
        assert_eq!(parsed.stop_sound.as_deref(), Some("system:Ping"));
        // 幂等
        let twice = render_notify_hooks_text(&rendered, &settings, &sounds_dir()).unwrap();
        assert_eq!(rendered, twice);
    }
}
