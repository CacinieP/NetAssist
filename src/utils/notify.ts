import { sendNotification, isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";

/**
 * Fire a native OS desktop notification (silently no-ops if the platform
 * denies permission). Used by network-abnormal and traffic-limit alerts.
 */
export async function notify(title: string, body: string): Promise<void> {
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      const perm = await requestPermission();
      granted = perm === "granted";
    }
    if (!granted) return;
    await sendNotification({ title, body });
  } catch (e) {
    // Non-fatal: notifications are best-effort.
    console.warn("Notification failed:", e);
  }
}
