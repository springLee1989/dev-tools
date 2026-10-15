import { invoke } from "@tauri-apps/api/core";
import type { AppStateView, SimInstance } from "./types";

export const api = {
  getState: () => invoke<AppStateView>("get_state"),
  loadProfile: (path: string) => invoke<AppStateView>("load_profile", { path }),
  loadProfileJson: (data: string) =>
    invoke<AppStateView>("load_profile_json", { data }),
  saveProfile: (path?: string | null) =>
    invoke<AppStateView>("save_profile", { path: path ?? null }),
  dumpProfile: () => invoke<string>("dump_profile"),
  setProfileMeta: (name: string, description: string) =>
    invoke<AppStateView>("set_profile_meta", { name, description }),
  addInstance: (instanceType: string) =>
    invoke<AppStateView>("add_instance", { instanceType }),
  removeInstance: (instanceId: string) =>
    invoke<AppStateView>("remove_instance", { instanceId }),
  cloneInstance: (instanceId: string) =>
    invoke<AppStateView>("clone_instance", { instanceId }),
  updateInstance: (instance: SimInstance) =>
    invoke<AppStateView & { warning?: string | null }>("update_instance", { instance }),
  startInstance: (instanceId: string) =>
    invoke<AppStateView>("start_instance", { instanceId }),
  stopInstance: (instanceId: string) =>
    invoke<AppStateView>("stop_instance", { instanceId }),
  startAll: () => invoke<AppStateView>("start_all"),
  stopAll: () => invoke<AppStateView>("stop_all"),
  triggerOnce: (instanceId: string, ruleName?: string | null) =>
    invoke<void>("trigger_once", {
      instanceId,
      ruleName: ruleName ?? null,
    }),
};
