import { useEffect, useState } from "react";
import {
  defaultGeneralSettings,
  getGeneralSettings,
  type GeneralSettings,
} from "./api/general";
import { DesktopShell } from "./app/DesktopShell";
import { applyTheme, normalizeGeneralSettings } from "./features/settings/generalSettings";

export function App() {
  const [settings, setSettings] = useState<GeneralSettings | null>(null);
  const [loadError, setLoadError] = useState("");

  useEffect(() => {
    const preventNativeContextMenu = (event: MouseEvent) => {
      event.preventDefault();
    };
    document.addEventListener("contextmenu", preventNativeContextMenu);
    return () => document.removeEventListener("contextmenu", preventNativeContextMenu);
  }, []);

  useEffect(() => {
    let active = true;
    void getGeneralSettings()
      .then((value) => {
        if (!active) return;
        const normalized = normalizeGeneralSettings(value);
        applyTheme(normalized.theme);
        setSettings(normalized);
      })
      .catch((error) => {
        if (!active) return;
        const fallback = defaultGeneralSettings();
        applyTheme(fallback.theme);
        setSettings(fallback);
        setLoadError(`General settings could not be loaded: ${String(error)}`);
      });
    return () => { active = false; };
  }, []);

  if (settings === null) return <div className="app-loading">Loading settings...</div>;
  return (
    <DesktopShell
      generalSettings={settings}
      generalSettingsLoadError={loadError}
      onGeneralSettingsChange={(next) => {
        applyTheme(next.theme);
        setSettings(next);
      }}
    />
  );
}
