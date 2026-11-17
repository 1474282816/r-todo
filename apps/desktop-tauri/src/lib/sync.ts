import {
  clearSyncCredentials,
  createPairingCode,
  getSyncStatus,
  listDevices,
  logoutSync,
  pairSync,
  revokeDevice,
  syncNow,
} from "./engine";
import type { Device, SyncStatus } from "./types";

export type SyncListener = (status: SyncStatus) => void;

let status: SyncStatus = {
  paired: false,
  connected: false,
  server_url: null,
  device_id: null,
  device_name: null,
  last_sync_at: null,
  error: null,
};

const listeners = new Set<SyncListener>();

function publish(next: SyncStatus): SyncStatus {
  status = next;
  for (const listener of listeners) listener(status);
  return status;
}

export function getStatus(): SyncStatus {
  return status;
}

export function subscribeStatus(listener: SyncListener): () => void {
  listeners.add(listener);
  listener(status);
  return () => listeners.delete(listener);
}

export async function restoreSession(): Promise<void> {
  await refreshStatus();
  if (status.paired) {
    await syncNow().catch(() => undefined);
  }
}

export async function refreshStatus(): Promise<SyncStatus> {
  return publish(await getSyncStatus());
}

export async function pair(
  serverUrl: string,
  pairingCode: string,
  deviceName: string,
): Promise<SyncStatus> {
  return publish(await pairSync(serverUrl, pairingCode, deviceName));
}

export async function fullSync(): Promise<SyncStatus> {
  return publish(await syncNow());
}

export async function logout(): Promise<SyncStatus> {
  const status = await logoutSync();
  await clearSyncCredentials();
  return publish(status);
}

export async function devices(): Promise<Device[]> {
  return listDevices();
}

export async function revoke(deviceId: string): Promise<Device[]> {
  await revokeDevice(deviceId);
  return listDevices();
}

export async function newPairingCode(): Promise<{ code: string; expires_at: string }> {
  return createPairingCode();
}
