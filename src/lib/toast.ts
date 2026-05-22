import { writeText } from "@tauri-apps/plugin-clipboard-manager";

export async function copyIp(ip: string): Promise<void> {
  await writeText(ip);
}
