import {
  Activity,
  ArrowDownUp,
  ArrowLeft,
  ArrowRight,
  Code,
  Database,
  DatabaseSearch,
  FileClock,
  Folder,
  FolderClock,
  Image as ImageIcon,
  Images,
  Link,
  MapPinned,
  Network,
  Search,
  Settings,
  TableProperties,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  getWorkspaceState,
  saveWorkspaceState,
  type GeneralSettings,
} from "../api/general";
import type { OperationState } from "../api/tasks";
import { errorMessage } from "../api/common";
import { cancelActiveTabTasks } from "../api/activeTasks";
import {
  getPhoto,
  type Photo,
} from "../api/photos";
import {
  getDatabaseLocations,
  listPhotoLibraries,
  openTaxonomyDatabase,
  openPhotoLibrary,
  type PhotoLibraryWorkspace,
} from "../api/storage";
import { selectPhotoDirectory, selectSqliteDatabase } from "../api/dialogs";
import { Button, EmptyState, IconButton, type IconComponent } from "../shared/ui";
import { MappingEditor } from "../features/mapping/MappingEditor";
import {
  createNavigationHistory,
  findNavigationTarget,
  pruneNavigationHistory,
  recordNavigation,
} from "./navigationHistory";
import { PhotoDetailView } from "../features/photos/PhotoDetailView";
import { PhotoLibraryIdentityProvider } from "../features/photos/PhotoLibraryIdentity";
import { PhotoSet } from "../features/photos/PhotoSet";
import { EmptyWorkspace } from "../features/photos/search/EmptyWorkspace";
import { GlobalSearchOverlay } from "../features/photos/search/GlobalSearchOverlay";
import {
  addRecentSearch,
  loadRecentSearches,
  normalizeSearchQuery,
  removeRecentSearch,
  saveRecentSearches,
  trimRecentSearches,
} from "../features/photos/search/recentSearchStorage";
import type { PhotoOpenHandlers } from "../features/photos/PhotoInteraction";
import { PhotoFullscreenPresentation } from "../features/photos/PhotoFullscreenPresentation";
import { setPhotoFullscreenActive } from "../features/photos/photoFullscreenState";
import { emitPhotoMutation, usePhotoMutation } from "../features/photos/photoMutations";
import {
  FolderPhotosView,
  PhotoMapView,
  TaxonPhotosView,
} from "../features/photos/PhotosView";
import { MappingView } from "../features/mapping/MappingView";
import { SettingsView, type SettingsSection } from "../features/settings/SettingsView";
import {
  FormattedUpdateView,
  TaxonomySearchView,
} from "../features/taxonomy/TaxonomyView";
import { TaxonomyHierarchyPage } from "../features/taxonomy/TaxonomyHierarchyPage";
import { CustomSqlView } from "../features/taxonomy/CustomSqlView";
import { OperationHistoryView } from "../features/operations/OperationHistoryView";
import { useOperationObserver } from "./useOperationObserver";
import { BackgroundTasks } from "./BackgroundTasks";
import { backgroundActiveCount } from "./backgroundPresentation";
import { ViewStateProvider, type ViewStateStore } from "../shared/viewState";
import {
  dependsOnReplacedTaxonomy,
  retainTabsAfterTaxonomyReplacement,
} from "./taxonomyReplacement";
import {
  closeAllTabsState,
  closeTabState,
  getCurrentTabStatus,
  pruneTabStatuses,
  updateTabStatus,
  type TabStatusMap,
} from "./tabState";
import { nativeMenuActions, useNativeMenu } from "./nativeMenu";
import { NativeAboutOverlay } from "./NativeAboutOverlay";
import { getTaxonDetail } from "../api/taxonomy";
import { TaxonDisplayPath } from "../features/taxonomy/TaxonDisplayPath";
import { MappingBadge } from "../features/mapping/MappingBadge";
import {
  statusBarMappingStatus,
  type PhotoTaxonDisplayState,
} from "../features/photos/photoTaxonDisplayState";
import {
  restoreWorkspaceState,
  serializeWorkspaceState,
  type AppTab,
} from "./workspaceState";
import { getTabName } from "./tabPresentation";
import { latestOperationForModule, operationByTaskId } from "./operationRegistry";

type TabKind = AppTab["kind"];

const initialTab: AppTab = { id: "folders", kind: "folders", title: "Folders" };
const keepAliveTabKinds = new Set<TabKind>([
  "map",
  "formatted-update",
  "custom-sql",
  "settings",
  "mapping",
]);

const photoItems: Array<[TabKind, string, IconComponent]> = [
  ["folders", "Folders", Folder],
  ["photo-taxonomy", "Taxon Tree", Network],
  ["map", "Map", MapPinned],
  ["photo-history", "Rename history", FolderClock],
];
const taxonomyItems: Array<[TabKind, string, IconComponent]> = [
  ["taxonomy-search", "Taxonomy search", DatabaseSearch],
  ["formatted-update", "Formatted update", TableProperties],
  ["custom-sql", "Custom SQL", Code],
  ["taxonomy-history", "Update history", FileClock],
];

const tabIcons: Record<TabKind, IconComponent> = {
  folders: Folder,
  "photo-taxonomy": Network,
  map: MapPinned,
  "photo-history": FolderClock,
  mapping: ArrowDownUp,
  "taxonomy-search": DatabaseSearch,
  "formatted-update": TableProperties,
  "custom-sql": Code,
  "taxonomy-history": FileClock,
  settings: Settings,
  "search-photos": Search,
  "taxon-photos": Images,
  "photo-detail": ImageIcon,
  "mapping-editor": Link,
  "taxon-detail": Database,
};

const photoTabKinds = new Set<TabKind>([
  "folders",
  "photo-taxonomy",
  "map",
  "photo-history",
  "mapping",
  "search-photos",
  "taxon-photos",
  "photo-detail",
  "mapping-editor",
]);

export function DesktopShell({
  generalSettings,
  generalSettingsLoadError,
  onGeneralSettingsChange,
}: {
  generalSettings: GeneralSettings;
  generalSettingsLoadError?: string;
  onGeneralSettingsChange: (settings: GeneralSettings) => void;
}) {
  const [tabs, setTabs] = useState<AppTab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [libraries, setLibraries] = useState<PhotoLibraryWorkspace[]>([]);
  const [workspaceReady, setWorkspaceReady] = useState(false);
  const [operationsOpen, setOperationsOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [fullscreenPhoto, setFullscreenPhoto] = useState<Photo | null>(null);
  const fullscreenReturnFocusRef = useRef<(() => void) | null>(null);
  const fullscreenRequestRef = useRef(0);
  const [searchOpen, setSearchOpen] = useState(false);
  const [recentSearches, setRecentSearches] = useState(() => (
    loadRecentSearches(undefined, generalSettings.recent_searches_limit)
  ));
  const [tabStatuses, setTabStatuses] = useState<TabStatusMap>({});
  const [photoTaxonDisplayStates, setPhotoTaxonDisplayStates] = useState<Record<string, PhotoTaxonDisplayState | null>>({});
  const [startedPhotoOperation, setStartedPhotoOperation] = useState<OperationState | null>(null);
  const startedPhotoOperationTabId = useRef<string | null>(null);
  const emptySearchInputRef = useRef<HTMLInputElement>(null);
  const operationsMenuRef = useRef<HTMLDivElement>(null);
  const activeIdRef = useRef<string | null>(null);
  const viewStateStores = useRef(new globalThis.Map<string, ViewStateStore>());
  const taskOwnerIds = useRef(new globalThis.Map<string, string>());
  const [navigationHistory, setNavigationHistory] = useState(
    createNavigationHistory(null),
  );
  const operations = useOperationObserver();
  const active = activeId === null ? null : tabs.find((tab) => tab.id === activeId) ?? null;
  const activeLibrary = libraries.find((library) => library.active) ?? null;
  const workspaceAvailable = Boolean(
    activeLibrary?.root_available && activeLibrary.database_available,
  );
  const runningOperations = Object.values(operations).filter((operation) => (
    operation.state === "queued" || operation.state === "running"
  ));
  const activeOperationCount = backgroundActiveCount(operations);
  const latestPhotoOperation = latestOperationForModule(operations, "photos");
  const latestMappingOperation = latestOperationForModule(operations, "mapping");
  const observedStartedPhotoOperation = operationByTaskId(
    operations,
    startedPhotoOperation?.task_id ?? null,
  ) ?? startedPhotoOperation;
  const photoLibraryOperation = observedStartedPhotoOperation
    && ["queued", "running"].includes(observedStartedPhotoOperation.state)
    ? observedStartedPhotoOperation
    : latestPhotoOperation && ["queued", "running"].includes(latestPhotoOperation.state)
      ? latestPhotoOperation
      : latestMappingOperation && ["queued", "running"].includes(latestMappingOperation.state)
        ? latestMappingOperation
        : null;
  const photoLibraryOperationError = latestPhotoOperation
    && ["photo_scan", "metadata_index"].includes(latestPhotoOperation.operation ?? "")
    && latestPhotoOperation.error
    ? `Photo Library indexing failed: ${latestPhotoOperation.error}. Retry the active library or reopen it.`
    : "";
  const taxonomyMutationLocked = runningOperations.some(
    (operation) => operation.operation === "apply_sql_import" || operation.operation === "apply_direct_import",
  );
  const existingTabIds = useMemo(
    () => new Set(tabs.map((tab) => tab.id)),
    [tabs],
  );
  const backTarget = findNavigationTarget(navigationHistory, existingTabIds, -1);
  const forwardTarget = findNavigationTarget(navigationHistory, existingTabIds, 1);
  const status = getCurrentTabStatus(tabStatuses, activeId);
  const activePhotoTaxonDisplayState = activeId === null ? null : photoTaxonDisplayStates[activeId] ?? null;
  const activePhotoTaxonSummary = activePhotoTaxonDisplayState?.summary ?? null;
  const activePhotoMappingStatus = statusBarMappingStatus(activePhotoTaxonDisplayState);

  const reportTabStatus = useCallback((tabId: string, message: string) => {
    setTabStatuses((current) => updateTabStatus(current, tabId, message));
  }, []);

  const reportPhotoTaxonDisplayState = useCallback((tabId: string, state: PhotoTaxonDisplayState | null) => {
    setPhotoTaxonDisplayStates((current) => current[tabId] === state
      ? current
      : { ...current, [tabId]: state });
  }, []);

  const reportActiveStatus = useCallback((message: string) => {
    const tabId = activeIdRef.current;
    if (tabId !== null) reportTabStatus(tabId, message);
  }, [reportTabStatus]);

  const followPhotoOperation = useCallback((operation: OperationState | null) => {
    if (!operation?.task_id) return;
    startedPhotoOperationTabId.current = activeIdRef.current;
    setStartedPhotoOperation(operation);
  }, []);

  useEffect(() => {
    if (!startedPhotoOperation?.task_id) return;
    const observed = operationByTaskId(operations, startedPhotoOperation.task_id);
    if (!observed || observed.state === "queued" || observed.state === "running") return;
    if (observed.error && startedPhotoOperationTabId.current !== null) {
      reportTabStatus(
        startedPhotoOperationTabId.current,
        `Photo Library task failed: ${observed.error}. Retry it in Settings or reopen it.`,
      );
    }
    setStartedPhotoOperation(null);
    startedPhotoOperationTabId.current = null;
  }, [operations, reportTabStatus, startedPhotoOperation?.task_id]);

  usePhotoMutation((mutation) => {
    if (mutation.kind !== "photo") return;
    const applyPhoto = (photo: Photo) => setTabs((current) => current.map((tab) => {
      if (tab.photo?.photo_id !== photo.photo_id) return tab;
      const title = tab.kind === "photo-detail"
        ? photo.filename
        : tab.kind === "mapping-editor"
          ? photo.filename
          : tab.title;
      return { ...tab, photo, title };
    }));
    if (mutation.photo) {
      applyPhoto(mutation.photo);
    } else {
      const photoIds = mutation.photoIds ?? (mutation.photoId === null ? [] : [mutation.photoId]);
      photoIds.forEach((photoId) => {
        void getPhoto(photoId).then(applyPhoto);
      });
    }
  });

  useEffect(() => {
    let cancelled = false;
    async function bootstrap() {
      let nextLibraries: PhotoLibraryWorkspace[] = [];
      try {
        nextLibraries = await listPhotoLibraries();
      } catch (nextError) {
        if (!cancelled) reportActiveStatus(String(nextError));
      }
      if (cancelled) return;
      setLibraries(nextLibraries);
      if (generalSettings.restore_tabs) {
        try {
          const activeLibrary = nextLibraries.find((library) => library.active) ?? null;
          const restored = await restoreWorkspaceState(await getWorkspaceState(), {
            getPhoto,
            photoWorkspaceAvailable: Boolean(
              activeLibrary?.root_available && activeLibrary.database_available,
            ),
            taxonExists: async (taxonId) => {
              try {
                await getTaxonDetail(taxonId);
                return true;
              } catch {
                return false;
              }
            },
          });
          if (cancelled) return;
          setTabs(restored.tabs);
          setActiveId(restored.activeId);
          setNavigationHistory(createNavigationHistory(restored.activeId));
        } catch (nextError) {
          if (!cancelled) reportActiveStatus(`Workspace state could not be restored: ${String(nextError)}`);
        }
      }
      if (!cancelled) setWorkspaceReady(true);
    }
    void bootstrap();
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    const next = trimRecentSearches(
      recentSearches,
      generalSettings.recent_searches_limit,
    );
    if (next.length === recentSearches.length) return;
    setRecentSearches(next);
    saveRecentSearches(next);
  }, [generalSettings.recent_searches_limit, recentSearches]);

  useEffect(() => {
    if (!workspaceReady || !generalSettings.restore_tabs) return;
    const timer = window.setTimeout(() => {
      void saveWorkspaceState(serializeWorkspaceState(tabs, activeId))
        .catch((nextError) => reportActiveStatus(`Workspace state could not be saved: ${String(nextError)}`));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [activeId, generalSettings.restore_tabs, tabs, workspaceReady]);

  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  useEffect(() => {
    setTabStatuses((current) => pruneTabStatuses(current, existingTabIds));
  }, [existingTabIds]);

  useEffect(() => {
    setNavigationHistory((current) => pruneNavigationHistory(
      current,
      existingTabIds,
      active?.id ?? null,
    ));
  }, [active?.id, existingTabIds]);

  useEffect(() => {
    if (!operationsOpen) return;
    const closeMenuOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target as Node;
      if (operationsOpen && !operationsMenuRef.current?.contains(target)) {
        setOperationsOpen(false);
      }
    };
    window.addEventListener("pointerdown", closeMenuOnOutsidePointer);
    return () => window.removeEventListener("pointerdown", closeMenuOnOutsidePointer);
  }, [operationsOpen]);

  function openTab(tab: AppTab, singleton = false) {
    const existing = tabs.find((item) => item.id === tab.id || (singleton && item.kind === tab.kind));
    if (existing) {
      if (tab.kind === "settings" && tab.settingsSection !== undefined) {
        setTabs((current) => current.map((item) => item.id === existing.id
          ? { ...item, settingsSection: tab.settingsSection }
          : item));
      } else if (tab.kind === "search-photos" && tab.query !== undefined) {
        setTabs((current) => current.map((item) => item.id === existing.id
          ? {
            ...item,
            title: tab.title,
            query: tab.query,
            refreshKey: (item.refreshKey ?? 0) + 1,
          }
          : item));
      }
      focusTab(existing.id);
      return;
    }
    setTabs((current) => [...current, tab]);
    focusTab(tab.id);
  }

  function taskOwnerId(tabId: string) {
    const existing = taskOwnerIds.current.get(tabId);
    if (existing) return existing;
    const ownerId = `${tabId}:${crypto.randomUUID()}`;
    taskOwnerIds.current.set(tabId, ownerId);
    return ownerId;
  }

  function cancelTabTasks(tabId: string) {
    const ownerId = taskOwnerIds.current.get(tabId);
    taskOwnerIds.current.delete(tabId);
    if (ownerId) void cancelActiveTabTasks(ownerId).catch(() => undefined);
  }

  function focusTab(id: string | null, record = true) {
    setActiveId(id);
    if (record && id !== null) {
      setNavigationHistory((current) => recordNavigation(current, id));
    }
  }

  function navigate(offset: -1 | 1) {
    const target = findNavigationTarget(navigationHistory, existingTabIds, offset);
    if (!target) return;
    setNavigationHistory((current) => ({ ...current, index: target.index }));
    focusTab(target.tabId, false);
  }

  function openModule(kind: TabKind, title: string) {
    openTab({ id: kind, kind, title }, true);
  }

  function updateTaxonTab(id: string, taxonId: number, title: string) {
    setTabs((current) => current.map((tab) => tab.id === id && tab.kind === "taxon-detail"
      ? { ...tab, taxonId, title }
      : tab));
  }

  function updateSettingsTab(id: string, settingsSection: SettingsSection) {
    setTabs((current) => current.map((tab) => tab.id === id && tab.kind === "settings"
      ? { ...tab, settingsSection }
      : tab));
  }

  function closeTab(id: string) {
    const remainingIds = new Set(tabs
      .filter((tab) => tab.id !== id)
      .map((tab) => tab.id));
    const previousActiveId = activeId === id
      ? findNavigationTarget(navigationHistory, remainingIds, -1)?.tabId ?? null
      : null;
    const next = closeTabState(tabs, activeId, id, previousActiveId);
    if (next.tabs === tabs) return;
    cancelTabTasks(id);
    viewStateStores.current.delete(id);
    setTabs(next.tabs);
    if (activeId !== next.activeId) focusTab(next.activeId);
    if (next.activeId === null) setSearchOpen(false);
  }

  const openPhotoFullscreen = useCallback((photo: Photo, onReturnFocus?: () => void) => {
    fullscreenReturnFocusRef.current = onReturnFocus ?? null;
    const requestId = ++fullscreenRequestRef.current;
    setFullscreenPhoto(photo);
    setPhotoFullscreenActive(true);
    window.requestAnimationFrame(() => {
      if (requestId !== fullscreenRequestRef.current) return;
      void getCurrentWindow().setFullscreen(true).catch((nextError) => {
        if (requestId !== fullscreenRequestRef.current) return;
        setPhotoFullscreenActive(false);
        setFullscreenPhoto(null);
        fullscreenReturnFocusRef.current = null;
        reportActiveStatus(errorMessage(nextError));
      });
    });
  }, [reportActiveStatus]);

  const exitPhotoFullscreen = useCallback(async () => {
    const requestId = fullscreenRequestRef.current;
    const appWindow = getCurrentWindow();
    let unlisten: (() => void) | null = null;
    let completed = false;
    const cleanup = () => {
      unlisten?.();
      unlisten = null;
    };
    const restoreAfterExit = () => {
      if (completed || requestId !== fullscreenRequestRef.current) {
        cleanup();
        return;
      }
      completed = true;
      cleanup();
      setPhotoFullscreenActive(false);
      setFullscreenPhoto(null);
      ++fullscreenRequestRef.current;
      window.requestAnimationFrame(() => {
        const restore = fullscreenReturnFocusRef.current;
        fullscreenReturnFocusRef.current = null;
        restore?.();
      });
    };
    try {
      unlisten = await appWindow.onResized(() => {
        void appWindow.isFullscreen()
          .then((fullscreen) => {
            if (fullscreen) return;
            restoreAfterExit();
          })
          .catch((nextError) => {
            if (completed || requestId !== fullscreenRequestRef.current) {
              cleanup();
              return;
            }
            completed = true;
            cleanup();
            reportActiveStatus(errorMessage(nextError));
          });
      });
      if (completed || requestId !== fullscreenRequestRef.current) {
        cleanup();
        return;
      }
      await appWindow.setFullscreen(false);
    } catch (nextError) {
      cleanup();
      if (requestId !== fullscreenRequestRef.current) return;
      completed = true;
      reportActiveStatus(errorMessage(nextError));
    }
  }, [reportActiveStatus]);

  const handlers: PhotoOpenHandlers = useMemo(() => ({
    openDetails: (photo) => openTab({ id: `photo:${photo.photo_id}`, kind: "photo-detail", title: photo.filename, photo }),
    openTaxon: (taxonId) => openTab({ id: `taxon-detail:${crypto.randomUUID()}`, kind: "taxon-detail", title: String(taxonId), taxonId }),
    openMappingEditor: (photo) => openTab({ id: `mapping:${photo.photo_id}`, kind: "mapping-editor", title: photo.filename, photo }),
    openFullscreen: (photo, onReturnFocus) => openPhotoFullscreen(photo, onReturnFocus),
  }), [openPhotoFullscreen, tabs]);

  async function submitSearch(query: string) {
    const value = normalizeSearchQuery(query);
    if (!value) return;
    if (!workspaceAvailable) throw new Error("Photo Library unavailable");
    openTab({ id: `search:${value.toLocaleLowerCase()}`, kind: "search-photos", title: `Search: ${value}`, query: value });
    setRecentSearches((current) => {
      const next = addRecentSearch(
        current,
        value,
        generalSettings.recent_searches_limit,
      );
      saveRecentSearches(next);
      return next;
    });
    setSearchOpen(false);
  }

  function deleteRecentSearch(query: string) {
    setRecentSearches((current) => {
      const next = removeRecentSearch(current, query);
      saveRecentSearches(next);
      return next;
    });
  }

  function clearRecentSearches() {
    setRecentSearches([]);
    saveRecentSearches([]);
  }

  function openGlobalSearch() {
    if (tabs.length === 0) {
      setSearchOpen(false);
      window.requestAnimationFrame(() => emptySearchInputRef.current?.focus());
      return;
    }
    setSearchOpen(true);
  }

  function resetPhotoWorkspace(message: string) {
    const statusTargetId = active !== null && !photoTabKinds.has(active.kind)
      ? active.id
      : initialTab.id;
    setTabs((current) => {
      const remaining = current.filter((tab) => !photoTabKinds.has(tab.kind));
      current.filter((tab) => photoTabKinds.has(tab.kind)).forEach((tab) => {
        cancelTabTasks(tab.id);
        viewStateStores.current.delete(tab.id);
      });
      if (remaining.length === 0 || (active !== null && photoTabKinds.has(active.kind))) {
        const folders = { ...initialTab };
        setActiveId(folders.id);
        return [...remaining, folders];
      }
      return remaining;
    });
    setSearchOpen(false);
    reportTabStatus(statusTargetId, message);
  }

  async function reloadLibraries() {
    try {
      setLibraries(await listPhotoLibraries());
    } catch (nextError) {
      reportActiveStatus(String(nextError));
    }
  }

  async function createLibrary(): Promise<boolean> {
    const selected = await selectPhotoDirectory();
    if (!selected) return false;
    try {
      const activation = await openPhotoLibrary(selected);
      followPhotoOperation(activation.operation);
      await reloadLibraries();
      resetPhotoWorkspace("Photo Library opened");
      return true;
    } catch (nextError) {
      reportActiveStatus(String(nextError));
      return false;
    }
  }

  async function openExistingTaxonomyDatabase() {
    try {
      const locations = await getDatabaseLocations();
      const selected = await selectSqliteDatabase(locations.default_taxonomy_directory);
      if (!selected) return;
      await openTaxonomyDatabase(selected);
      resetTaxonomyResources("Taxonomy Database opened. Photo mappings are being rebuilt in the background.");
    } catch (nextError) {
      reportActiveStatus(String(nextError));
    }
  }

  function closeAllTabs() {
    const next = closeAllTabsState<AppTab>();
    tabs.forEach((tab) => cancelTabTasks(tab.id));
    viewStateStores.current.clear();
    setTabStatuses({});
    setTabs(next.tabs);
    focusTab(next.activeId);
    setOperationsOpen(false);
    setAboutOpen(false);
    setSearchOpen(false);
  }

  function resetTaxonomyResources(message = "Taxonomy database replaced successfully. Photo mappings are being rebuilt in the background.") {
    tabs.filter((tab) => dependsOnReplacedTaxonomy(tab.kind))
      .forEach((tab) => {
        cancelTabTasks(tab.id);
        viewStateStores.current.delete(tab.id);
      });
    const remaining = retainTabsAfterTaxonomyReplacement(tabs);
    const nextTabs = remaining.length > 0 ? remaining : [{ ...initialTab }];
    const nextActiveId = nextTabs.some((tab) => tab.id === activeId)
      ? activeId
      : nextTabs[0]?.id ?? null;
    setTabs(nextTabs);
    if (nextActiveId !== activeId) setActiveId(nextActiveId);
    emitPhotoMutation({ photoId: null, kind: "mapping" });
    if (nextActiveId !== null) reportTabStatus(nextActiveId, message);
  }

  useNativeMenu((action) => {
    if (action === nativeMenuActions.aboutVividarium) {
      setSearchOpen(false);
      setAboutOpen(true);
    }
    if (action === nativeMenuActions.openPhotoLibrary) void createLibrary();
    if (action === nativeMenuActions.managePhotoLibraries) {
      openTab({ id: "settings", kind: "settings", title: "Settings", settingsSection: "Photo Libraries" }, true);
    }
    if (action === nativeMenuActions.openTaxonomyDatabase) void openExistingTaxonomyDatabase();
    if (action === nativeMenuActions.manageTaxonomyDatabases) {
      openTab({ id: "settings", kind: "settings", title: "Settings", settingsSection: "Taxonomy Databases" }, true);
    }
    if (action === nativeMenuActions.closeAllTabs) closeAllTabs();
  });

  return (
    <div className="desktop-shell">
      <aside className="activity-bar">
        <ActivityButton icon={Search} label="Search photos" active={searchOpen || active === null} onClick={openGlobalSearch} />
        <div className="activity-divider" />
        {photoItems.map(([kind, label, icon]) => <ActivityButton key={kind} icon={icon} label={label} active={active?.kind === kind} disabled={!workspaceAvailable} onClick={() => openModule(kind, label)} />)}
        <div className="activity-divider" />
        <ActivityButton icon={ArrowDownUp} label="Mapping" active={active?.kind === "mapping"} disabled={!workspaceAvailable} onClick={() => openModule("mapping", "Mapping")} />
        <div className="activity-divider" />
        {taxonomyItems.map(([kind, label, icon]) => <ActivityButton key={kind} icon={icon} label={label} active={active?.kind === kind} onClick={() => openModule(kind, label)} />)}
        <div className="activity-spacer" />
        <ActivityButton icon={Settings} label="Settings" active={active?.kind === "settings"} onClick={() => openModule("settings", "Settings")} />
      </aside>
      <div className="desktop-main">
        <header className="app-topbar">
          <div className="toolbar-navigation">
            <IconButton aria-label="Go Back" title="Go Back" disabled={!backTarget} onClick={() => navigate(-1)}><ArrowLeft size={14} /></IconButton>
            <IconButton aria-label="Go Forward" title="Go Forward" disabled={!forwardTarget} onClick={() => navigate(1)}><ArrowRight size={14} /></IconButton>
          </div>
          <div className="tab-strip" role="tablist" aria-label="Open tabs">
            {tabs.map((tab) => {
              const name = getTabName(tab);
              const TabIcon = tabIcons[tab.kind];
              return (
                <div
                  aria-selected={tab.id === activeId}
                  className={`app-tab${tab.id === activeId ? " active" : ""}`}
                  key={tab.id}
                  role="tab"
                  tabIndex={0}
                  title={name}
                  onClick={() => focusTab(tab.id)}
                  onKeyDown={(event) => {
                    if (event.target !== event.currentTarget) return;
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      focusTab(tab.id);
                    }
                  }}
                >
                  <TabIcon aria-hidden="true" className="app-tab-icon" size={14} strokeWidth={1.8} />
                  <span className="app-tab-title">{name}</span>
                  <IconButton
                    aria-label={`Close ${name}`}
                    className="app-tab-close"
                    size="small"
                    title={`Close ${name}`}
                    onClick={(event) => {
                      event.stopPropagation();
                      closeTab(tab.id);
                    }}
                  ><X size={12} /></IconButton>
                </div>
              );
            })}
          </div>
        </header>
        <main className="tab-content">
          {workspaceReady && tabs.length === 0 && (
            <EmptyWorkspace
              inputRef={emptySearchInputRef}
              recentSearches={recentSearches}
              suggestionsEnabled={workspaceAvailable}
              onSubmit={submitSearch}
              onRemoveRecent={deleteRecentSearch}
              onClearRecent={clearRecentSearches}
            />
          )}
          {tabs.map((tab) => {
            const isActive = tab.id === activeId;
            if (!isActive && !keepAliveTabKinds.has(tab.kind)) return null;
            let viewState = viewStateStores.current.get(tab.id);
            if (!viewState) {
              viewState = new globalThis.Map();
              viewStateStores.current.set(tab.id, viewState);
            }
            return (
              <section
                className={`tab-panel${isActive ? " active" : ""}`}
                aria-hidden={!isActive}
                key={tab.id}
              >
                <PhotoLibraryIdentityProvider libraryUuid={activeLibrary?.library_uuid ?? null}>
                  <ViewStateProvider store={viewState}>
                    <TabBody
                      active={isActive}
                      tab={tab}
                      taskOwnerId={taskOwnerId(tab.id)}
                      handlers={handlers}
                      onTabStatus={reportTabStatus}
                      onPhotoTaxonDisplayState={reportPhotoTaxonDisplayState}
                      openTab={openTab}
                      updateTaxonTab={updateTaxonTab}
                      updateSettingsTab={updateSettingsTab}
                      workspaceAvailable={workspaceAvailable}
                      activeLibrary={activeLibrary}
                      taxonomyMutationLocked={taxonomyMutationLocked}
                      onOpenPhotoLibrary={createLibrary}
                      onPhotoOperationStarted={followPhotoOperation}
                      photoLibraryOperation={photoLibraryOperation}
                      photoLibraryOperationError={photoLibraryOperationError}
                      onWorkspaceChanged={async (resetPhotoTabs) => {
                        await reloadLibraries();
                        if (resetPhotoTabs) resetPhotoWorkspace("Photo Library workspace changed");
                      }}
                      onTaxonomyImported={resetTaxonomyResources}
                      generalSettings={generalSettings}
                      generalSettingsLoadError={generalSettingsLoadError}
                      onGeneralSettingsChange={onGeneralSettingsChange}
                    />
                  </ViewStateProvider>
                </PhotoLibraryIdentityProvider>
              </section>
            );
          })}
        </main>
        <footer className="status-bar">
          <span className="status-dot" />
          {status}
          {activePhotoTaxonSummary ? (
            <TaxonDisplayPath
              className="status-title"
              summary={activePhotoTaxonSummary}
              nameParts={generalSettings.photos_taxon_name_parts}
            />
          ) : activePhotoMappingStatus ? (
            <span className="status-title"><MappingBadge status={activePhotoMappingStatus} /></span>
          ) : (
            <span className="status-title">{active?.title ?? ""}</span>
          )}
          <div className="status-operations" ref={operationsMenuRef}>
            <IconButton aria-label="Background" className={activeOperationCount > 0 ? "running" : ""} title="Background" onClick={() => {
              setOperationsOpen((current) => !current);
            }}>
              <Activity size={12} /><span>{activeOperationCount}</span>
            </IconButton>
            {operationsOpen && (
              <BackgroundTasks operations={operations} />
            )}
          </div>
        </footer>
      </div>
      {searchOpen && active !== null && (
        <GlobalSearchOverlay
          suggestionsEnabled={workspaceAvailable}
          onClose={() => setSearchOpen(false)}
          onSubmit={submitSearch}
        />
      )}
      {aboutOpen && <NativeAboutOverlay onClose={() => setAboutOpen(false)} />}
      {fullscreenPhoto && (
        <PhotoLibraryIdentityProvider libraryUuid={activeLibrary?.library_uuid ?? null}>
          <PhotoFullscreenPresentation
            photo={fullscreenPhoto}
            onExit={exitPhotoFullscreen}
          />
        </PhotoLibraryIdentityProvider>
      )}
    </div>
  );
}

function TabBody({
  active,
  tab,
  taskOwnerId,
  handlers,
  onTabStatus,
  onPhotoTaxonDisplayState,
  openTab,
  updateTaxonTab,
  updateSettingsTab,
  workspaceAvailable,
  activeLibrary,
  taxonomyMutationLocked,
  onOpenPhotoLibrary,
  onPhotoOperationStarted,
  photoLibraryOperation,
  photoLibraryOperationError,
  onWorkspaceChanged,
  onTaxonomyImported,
  generalSettings,
  generalSettingsLoadError,
  onGeneralSettingsChange,
}: {
  active: boolean;
  tab: AppTab;
  taskOwnerId: string;
  handlers: PhotoOpenHandlers;
  onTabStatus: (tabId: string, message: string, busy?: boolean) => void;
  onPhotoTaxonDisplayState: (tabId: string, state: PhotoTaxonDisplayState | null) => void;
  openTab: (tab: AppTab, singleton?: boolean) => void;
  updateTaxonTab: (id: string, taxonId: number, title: string) => void;
  updateSettingsTab: (id: string, section: SettingsSection) => void;
  workspaceAvailable: boolean;
  activeLibrary: PhotoLibraryWorkspace | null;
  taxonomyMutationLocked: boolean;
  onOpenPhotoLibrary: () => Promise<boolean>;
  onPhotoOperationStarted: (operation: OperationState | null) => void;
  photoLibraryOperation: OperationState | null;
  photoLibraryOperationError: string;
  onWorkspaceChanged: (resetPhotoTabs: boolean) => Promise<void>;
  onTaxonomyImported: () => void;
  generalSettings: GeneralSettings;
  generalSettingsLoadError?: string;
  onGeneralSettingsChange: (settings: GeneralSettings) => void;
}) {
  const onStatus = useCallback(
    (message: string, busy?: boolean) => onTabStatus(tab.id, message, busy),
    [onTabStatus, tab.id],
  );
  const onCurrentPhotoTaxonDisplayState = useCallback(
    (state: PhotoTaxonDisplayState | null) => onPhotoTaxonDisplayState(tab.id, state),
    [onPhotoTaxonDisplayState, tab.id],
  );
  if (photoTabKinds.has(tab.kind) && !workspaceAvailable) {
    return (
      <EmptyState
        title={activeLibrary ? "Photo Library unavailable" : "No Photo Library selected"}
        detail={activeLibrary
          ? "Reconnect its photo root or database in Settings, then open the library again."
          : "Create or select a Photo Library from the top toolbar."}
        icon={Database}
        action={(
          <div className="empty-state-actions">
            {!activeLibrary && <Button variant="primary" onClick={() => void onOpenPhotoLibrary()}>Create or open</Button>}
            <Button onClick={() => openTab({ id: "settings", kind: "settings", title: "Settings", settingsSection: "Photo Libraries" }, true)}>Manage libraries</Button>
          </div>
        )}
      />
    );
  }
  if (tab.kind === "folders") return <FolderPhotosView active={active} handlers={handlers} onStatus={onStatus} onPhotoTaxonDisplayState={onCurrentPhotoTaxonDisplayState} backgroundOperation={photoLibraryOperation} />;
  if (tab.kind === "photo-taxonomy") return <TaxonPhotosView active={active} handlers={handlers} nameParts={generalSettings.photos_taxon_name_parts} onStatus={onStatus} onPhotoTaxonDisplayState={onCurrentPhotoTaxonDisplayState} backgroundOperation={photoLibraryOperation} />;
  if (tab.kind === "map") return <PhotoMapView active={active} handlers={handlers} onStatus={onStatus} onPhotoTaxonDisplayState={onCurrentPhotoTaxonDisplayState} backgroundOperation={photoLibraryOperation} />;
  if (tab.kind === "photo-history") return <OperationHistoryView domain="photo" onStatus={onStatus} />;
  if (tab.kind === "mapping") return <MappingView active={active} onStatus={onStatus} onPhotoTaxonDisplayState={onCurrentPhotoTaxonDisplayState} handlers={handlers} />;
  if (tab.kind === "taxonomy-search") return <TaxonomySearchView nameParts={generalSettings.taxonomy_taxon_name_parts} mutationDisabled={taxonomyMutationLocked} onStatus={onStatus} onOpenPhotos={(taxonId, label) => openTab({ id: `taxon-photos:${taxonId}`, kind: "taxon-photos", title: label, taxonId })} />;
  if (tab.kind === "taxon-detail" && tab.taxonId !== undefined) return <TaxonomyHierarchyPage initialTaxonId={tab.taxonId} nameParts={generalSettings.taxonomy_taxon_name_parts} mutationDisabled={taxonomyMutationLocked} onTaxonChange={(taxonId, label) => updateTaxonTab(tab.id, taxonId, label)} onOpenPhotos={(taxonId, label) => openTab({ id: `taxon-photos:${taxonId}`, kind: "taxon-photos", title: label, taxonId })} />;
  if (tab.kind === "formatted-update") return <FormattedUpdateView onStatus={onStatus} taskOwnerId={taskOwnerId} mutationDisabled={taxonomyMutationLocked} />;
  if (tab.kind === "custom-sql") return <CustomSqlView onStatus={onStatus} taskOwnerId={taskOwnerId} mutationDisabled={taxonomyMutationLocked} />;
  if (tab.kind === "taxonomy-history") return <OperationHistoryView domain="taxonomy" onStatus={onStatus} />;
  if (tab.kind === "settings") return <SettingsView section={tab.settingsSection ?? "General"} taskOwnerId={taskOwnerId} onStatus={onStatus} onSectionChange={(section) => updateSettingsTab(tab.id, section)} onTaxonomyImported={onTaxonomyImported} onWorkspaceChanged={onWorkspaceChanged} onOpenPhotoLibrary={onOpenPhotoLibrary} onPhotoOperationStarted={onPhotoOperationStarted} photoLibraryOperation={photoLibraryOperation} photoLibraryOperationError={photoLibraryOperationError} generalSettings={generalSettings} generalSettingsLoadError={generalSettingsLoadError} onGeneralSettingsChange={onGeneralSettingsChange} />;
  if (tab.kind === "photo-detail" && tab.photo) return <PhotoDetailView active={active} photo={tab.photo} handlers={handlers} onStatus={onStatus} onPhotoTaxonDisplayState={onCurrentPhotoTaxonDisplayState} />;
  if (tab.kind === "mapping-editor" && tab.photo) return <MappingEditor active={active} photo={tab.photo} onStatus={onStatus} onPhotoTaxonDisplayState={onCurrentPhotoTaxonDisplayState} handlers={handlers} />;
  if (tab.kind === "search-photos" && tab.query) return <PhotoSet active={active} query={tab.query} refreshKey={tab.refreshKey} handlers={handlers} onStatus={onStatus} onPhotoTaxonDisplayState={onCurrentPhotoTaxonDisplayState} />;
  if (tab.kind === "taxon-photos" && tab.taxonId !== undefined) return <PhotoSet active={active} taxonId={tab.taxonId} handlers={handlers} onStatus={onStatus} onPhotoTaxonDisplayState={onCurrentPhotoTaxonDisplayState} />;
  return null;
}

function ActivityButton({
  icon: Icon,
  label,
  active,
  onClick,
  disabled = false,
}: {
  icon: IconComponent;
  label: string;
  active: boolean;
  onClick: () => void;
  disabled?: boolean;
}) {
  return <IconButton className={`activity-button${active ? " active" : ""}`} title={label} aria-label={label} disabled={disabled} onClick={onClick}><Icon size={19} /></IconButton>;
}
