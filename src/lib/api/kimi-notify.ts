import { invoke } from "@tauri-apps/api/core";

export interface KimiNotifySettings {
  enabled: boolean;
  stopSound: string | null;
  taskCompletedSound: string | null;
  subagentStopSound: string | null;
}

export const kimiNotifyApi = {
  async listSounds(): Promise<string[]> {
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
