import { ipc, type DeviceInfo } from "../lib/ipc";
import { copyIp } from "../lib/toast";
import { flash, useStore } from "../store";
import { t } from "../lib/i18n";
import Tooltip from "./Tooltip";
import { IconCopy, IconGlobe, IconPing } from "./icons";

type Props = { devices: DeviceInfo[] };

/** "a.b.c.d" → numerieke sleutel, voor stabiele oplopende IP-volgorde. */
function ipKey(ip: string): number {
  return ip.split(".").reduce((acc, part) => acc * 256 + (parseInt(part, 10) || 0), 0);
}

/** Het label onder het IP: merk/product, of een generiek camera-label, of niets. */
function brandLabel(d: DeviceInfo): string | null {
  if (d.brand) return d.brand;
  if (d.is_camera) return t("network.cameraUnknown");
  return null;
}

/** Apparatenlijst: IP boven, merk/type eronder; rijen gescheiden door een dunne lijn.
 *  Altijd op numerieke IP-volgorde, dus nieuwe vondsten schuiven netjes op hun plek. */
export default function DeviceList({ devices }: Props) {
  if (devices.length === 0) return null;

  const sorted = [...devices].sort((a, b) => ipKey(a.ip) - ipKey(b.ip));
  const requestPing = useStore((s) => s.requestPing);

  const onCopy = async (ip: string) => {
    try {
      await copyIp(ip);
      flash(t("network.copied"), "ok");
    } catch {
      /* klembord niet beschikbaar — stil negeren */
    }
  };

  return (
    <ul className="device-list">
      {sorted.map((d) => {
        const label = brandLabel(d);
        const clickable = d.web_url != null;
        const open = () => {
          if (d.web_url) ipc.openExternal(d.web_url).catch(() => {});
        };
        return (
          <li
            key={d.ip}
            className={
              "device-row" +
              (d.is_camera ? " device-row--camera" : "") +
              (clickable ? " device-row--link" : "")
            }
            onClick={clickable ? open : undefined}
          >
            <div className="device-row__main">
              <span className="device-ip">{d.ip}</span>
              {d.web_url && (
                <span className="device-web" aria-label={t("network.openInBrowser")}>
                  <IconGlobe size={12} />
                </span>
              )}
              <span className="device-actions">
                <Tooltip text={t("network.pingDevice")} side="top" delay={300}>
                  <button
                    className="device-copy device-ping"
                    aria-label={t("network.pingDevice")}
                    onClick={(e) => {
                      e.stopPropagation();
                      requestPing(d.ip);
                    }}
                  >
                    <IconPing size={14} />
                  </button>
                </Tooltip>
                <Tooltip text={t("network.copyIp")} side="top" delay={300}>
                  <button
                    className="device-copy"
                    aria-label={t("network.copyIp")}
                    onClick={(e) => {
                      e.stopPropagation();
                      onCopy(d.ip);
                    }}
                  >
                    <IconCopy size={14} />
                  </button>
                </Tooltip>
              </span>
            </div>
            {label && <span className="device-brand">{label}</span>}
            {d.detail && <span className="device-detail">{d.detail}</span>}
          </li>
        );
      })}
    </ul>
  );
}
