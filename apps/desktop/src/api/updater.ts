import { Channel, invoke } from "@tauri-apps/api/core";
import { call, desktopRuntime } from "./client";

export type AppUpdateInfo = {
  current_version: string;
  version: string;
  notes: string | null;
  published_at: string | null;
};
export type AppUpdateEvent =
  | { event: "started"; data: { content_length: number | null } }
  | { event: "progress"; data: { chunk_length: number; downloaded: number } }
  | { event: "finished" };

export const getAppVersion = () => call<string>("get_app_version", undefined, () => "development");
export const checkAppUpdate = () => call<AppUpdateInfo | null>("check_app_update", undefined, () => null);
export async function installAppUpdate(onEvent: (event: AppUpdateEvent) => void): Promise<void> {
  if (!desktopRuntime) return;
  const onEventChannel = new Channel<AppUpdateEvent>();
  onEventChannel.onmessage = onEvent;
  await invoke("install_app_update", { onEvent: onEventChannel });
}
