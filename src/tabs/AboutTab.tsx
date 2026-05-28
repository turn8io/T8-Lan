import turn8LogoUrl from "../../logo/turn8-logo.svg?url";
import { ipc } from "../lib/ipc";
import { t } from "../lib/i18n";

const APP_VERSION = "0.2.0";
const GITHUB_URL = "https://github.com/turn8io/T8-Lan";
const SITE_URL = "https://www.turn8.io";
const TOOLBOX_URL = "https://www.321-test.com";

export default function AboutTab() {
  return (
    <section className="about">
      <button
        className="about__logo-btn"
        onClick={() => ipc.openExternal(SITE_URL)}
        aria-label="turn8.io"
        title="turn8.io"
      >
        <img src={turn8LogoUrl} alt="Turn8" className="about__logo" />
      </button>
      <p className="about__slogan">{t("about.slogan")}</p>

      <div className="about__divider" />

      <p className="about__line">
        <strong>T8-Lan</strong> v{APP_VERSION}
      </p>
      <p className="about__line about__muted">{t("about.description")}</p>

      <div className="about__links">
        <button className="link" onClick={() => ipc.openExternal(GITHUB_URL)}>
          {t("about.github")}
        </button>
      </div>

      <div className="about__toolbox">
        <span className="about__muted">{t("about.toolbox")}</span>
        <button className="link" onClick={() => ipc.openExternal(TOOLBOX_URL)}>
          www.321-test.com
        </button>
      </div>
    </section>
  );
}
