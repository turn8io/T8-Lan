import { ipc, isValidIpv4 } from "./ipc";

/**
 * Apply a static IP to an adapter, with an ARP-conflict confirmation.
 * Recents/DNS/gateway are handled by the backend. The success/error toast is
 * driven by the "switch-result" event, so callers should not toast themselves.
 * Throws "cancelled" if the user declines a conflict, or a backend error.
 */
export async function applyStaticIp(adapterName: string, ip: string): Promise<void> {
  if (!isValidIpv4(ip)) throw new Error(`Ongeldig IP: ${ip}`);

  const conflictMac = await ipc.checkIpConflict(ip).catch(() => null);
  if (conflictMac) {
    const ok = window.confirm(
      `IP ${ip} is al in gebruik door MAC ${conflictMac}. Toch toepassen?`,
    );
    if (!ok) throw new Error("cancelled");
  }
  await ipc.switchToStatic(adapterName, ip);
}

export async function applyDhcp(adapterName: string): Promise<void> {
  await ipc.switchToDhcp(adapterName);
}
