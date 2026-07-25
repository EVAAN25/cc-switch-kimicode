import { invoke } from "@tauri-apps/api/core";

export interface KimiNotifySettings {
  enabled: boolean;
  stopSound: string | null;
  taskCompletedSound: string | null;
  subagentStopSound: string | null;
}

/** 可用音效，按来源分组；system 仅 macOS 非空，选择时前端加 "system:" 前缀 */
export interface KimiNotifySounds {
  bundled: string[];
  system: string[];
}

export const KIMI_NOTIFY_SYSTEM_PREFIX = "system:";

export const kimiNotifyApi = {
  async listSounds(): Promise<KimiNotifySounds> {
    return await invoke("list_kimi_notify_sounds");
  },

  async preview(name: string): Promise<boolean> {
    return await invoke("preview_kimi_notify_sound", { name });
  },

  async getSettings(): Promise<KimiNotifySettings> {
    return await invoke("get_kimi_notify_settings");
  },

  async setSettings(settings: KimiNotifySettings): Promise<boolean> {
    return await invoke("set_kimi_notify_settings", { settings });
  },
};
