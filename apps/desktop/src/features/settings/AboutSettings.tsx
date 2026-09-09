import { useEffect, useState, type MouseEvent } from "react";
import { errorMessage } from "../../api/common";
import {
  authorEmail,
  authorEmailUrl,
  openExternalUrl,
  projectRepositoryUrl,
} from "../../api/external";
import {
  checkAppUpdate,
  getAppVersion,
  installAppUpdate,
  type AppUpdateInfo,
} from "../../api/updater";
import { Button, SectionHeader } from "../../shared/ui";

export function AboutSettings() {
  const [version, setVersion] = useState("Loading...");
  const [availableUpdate, setAvailableUpdate] = useState<AppUpdateInfo | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateMessage, setUpdateMessage] = useState("Updates are delivered from GitHub Releases.");
  const [updateError, setUpdateError] = useState("");
  const [linkError, setLinkError] = useState("");

  useEffect(() => { void getAppVersion().then(setVersion); }, []);

  async function checkUpdate() {
    setUpdateBusy(true);
    setUpdateError("");
    setUpdateMessage("Checking GitHub Releases...");
    try {
      const update = await checkAppUpdate();
      setAvailableUpdate(update);
      setUpdateMessage(update ? `Version ${update.version} is available.` : "Vividarium is up to date.");
    } catch (nextError) {
      setUpdateError(errorMessage(nextError));
    } finally {
      setUpdateBusy(false);
    }
  }

  async function installUpdate() {
    setUpdateBusy(true);
    setUpdateError("");
    setUpdateMessage("Preparing update...");
    try {
      await installAppUpdate((event) => {
        if (event.event === "started") setUpdateMessage("Downloading update...");
        if (event.event === "progress") setUpdateMessage(`Downloaded ${event.data.downloaded} bytes.`);
        if (event.event === "finished") setUpdateMessage("Installing and restarting...");
      });
    } catch (nextError) {
      setUpdateError(errorMessage(nextError));
      setUpdateBusy(false);
    }
  }

  function openLink(event: MouseEvent<HTMLAnchorElement>, url: string) {
    event.preventDefault();
    setLinkError("");
    void openExternalUrl(url).catch((nextError) => setLinkError(errorMessage(nextError)));
  }

  return (
    <div className="settings-section">
      <SectionHeader title="About" detail="View application, version, update, author, and project information." />
      <div className="about-settings">
        <strong>Vividarium</strong>
        <AboutValue label="Version" value={version} />
        <AboutValue label="Database schema" value="3" />
        <AboutValue label="Author" value="Yunfan Bao" />
        <div className="setting-row">
          <span>Email</span>
          <a href={authorEmailUrl} onClick={(event) => openLink(event, authorEmailUrl)}>{authorEmail}</a>
        </div>
        <div className="setting-row">
          <span>GitHub</span>
          <a href={projectRepositoryUrl} onClick={(event) => openLink(event, projectRepositoryUrl)}>github.com/baoyunfan0101/Vividarium</a>
        </div>
        {linkError && <div className="inline-error" role="alert">{linkError}</div>}
        <div className="about-update">
          <div><strong>Software update</strong><span>{updateMessage}</span></div>
          {availableUpdate ? (
            <Button variant="primary" disabled={updateBusy} onClick={() => void installUpdate()}>Install and restart</Button>
          ) : (
            <Button disabled={updateBusy} onClick={() => void checkUpdate()}>Check for updates</Button>
          )}
        </div>
        {updateError && <div className="inline-error" role="alert">{updateError}</div>}
      </div>
    </div>
  );
}

function AboutValue({ label, value }: { label: string; value: string }) {
  return <div className="setting-row"><span>{label}</span><strong>{value}</strong></div>;
}
