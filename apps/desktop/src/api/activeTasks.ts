import { call } from "./client";

export const cancelActiveTabTasks = (ownerId: string) =>
  call<number>("cancel_active_tab_tasks", { ownerId }, () => 0);
