import { invoke } from "@tauri-apps/api/core";

function call(name, body) {
  return invoke(name, body === undefined ? {} : { body });
}

export const backend = {
  health: () => call("health"),
  models: () => call("models"),
  modelSummary: () => call("model_summary"),
  sync: (body) => call("sync", body),
  remoteView: (body) => call("remote_view", body),
  remoteModels: (body) => call("remote_models", body),
  sshTest: (body) => call("ssh_test", body),
  autoDetect: () => call("auto_detect"),
  configCompare: (body) => call("config_compare", body),
  autoSyncStart: (body) => call("auto_sync_start", body),
  autoSyncStop: () => call("auto_sync_stop"),
  autoSyncStatus: () => call("auto_sync_status"),
};
