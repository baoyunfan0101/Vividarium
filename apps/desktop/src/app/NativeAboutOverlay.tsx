import { useEffect, useState } from "react";
import { getAppVersion } from "../api/updater";
import { errorMessage } from "../api/common";
import {
  authorEmail,
  authorEmailUrl,
  openExternalUrl,
  projectRepositoryUrl,
} from "../api/external";

export function NativeAboutOverlay({ onClose }: { onClose: () => void }) {
  const [version, setVersion] = useState("Loading...");
  const [linkError, setLinkError] = useState("");

  useEffect(() => {
    void getAppVersion().then(setVersion);
  }, []);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  function openLink(event: React.MouseEvent<HTMLAnchorElement>, url: string) {
    event.preventDefault();
    setLinkError("");
    void openExternalUrl(url).catch((nextError) => setLinkError(errorMessage(nextError)));
  }

  return (
    <div
      className="native-about-overlay"
      role="presentation"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="native-about-dialog" role="dialog" aria-label="About Vividarium" aria-modal="true">
        <h1>Vividarium</h1>
        <div><span>Version:</span><strong>{version}</strong></div>
        <div><span>Author:</span><strong>Yunfan Bao</strong></div>
        <div><span>Email:</span><a href={authorEmailUrl} onClick={(event) => openLink(event, authorEmailUrl)}>{authorEmail}</a></div>
        <div><span>GitHub:</span><a href={projectRepositoryUrl} onClick={(event) => openLink(event, projectRepositoryUrl)}>github.com/baoyunfan0101/Vividarium</a></div>
        {linkError && <div className="inline-error" role="alert">{linkError}</div>}
      </div>
    </div>
  );
}
