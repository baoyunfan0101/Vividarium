import { useState } from "react";
import { Database, Folder, GitBranch, Map, TreePine } from "lucide-react";
import { NavButton } from "./components/NavButton";
import { AdminPanel } from "./features/admin/AdminPanel";
import { MapPage } from "./features/map/MapPage";
import { PhotosExplorer } from "./features/photos/PhotosExplorer";
import { TaxonomyExplorer } from "./features/taxonomy/TaxonomyExplorer";
import { readStorage, writeStorage } from "./lib/storage";

type View = "photos" | "taxonomy" | "map" | "admin";
const APP_VIEW_KEY = "phytoindex.app.view";

export function App() {
  const [view, setView] = useState<View>(() => readStorage<View>(APP_VIEW_KEY, "photos"));
  const [, setMessage] = useState("Ready");

  function switchView(nextView: View) {
    setView(nextView);
    writeStorage(APP_VIEW_KEY, nextView);
  }

  return (
    <div className="shell">
      <header className="app-header">
        <div className="brand">
          <TreePine size={22} />
          <span>PhytoIndex</span>
        </div>
        <nav>
          <NavButton active={view === "photos"} icon={<Folder size={18} />} label="Photos" onClick={() => switchView("photos")} />
          <NavButton active={view === "taxonomy"} icon={<GitBranch size={18} />} label="Taxonomy" onClick={() => switchView("taxonomy")} />
          <NavButton active={view === "map"} icon={<Map size={18} />} label="Map" onClick={() => switchView("map")} />
          <NavButton active={view === "admin"} icon={<Database size={18} />} label="Admin" onClick={() => switchView("admin")} />
        </nav>
      </header>

      <main className="workspace">
        {view === "photos" && <PhotosExplorer setMessage={setMessage} />}
        {view === "taxonomy" && <TaxonomyExplorer setMessage={setMessage} />}
        {view === "map" && <MapPage />}
        {view === "admin" && <AdminPanel setMessage={setMessage} />}
      </main>
    </div>
  );
}
