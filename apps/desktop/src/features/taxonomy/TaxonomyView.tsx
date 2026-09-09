import {
  CircleQuestionMark,
  Download,
  Eye,
  FileUp,
  Play,
  Search,
  Trash2,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  applyTaxonomyRows,
  getTaxonomyTemplate,
  parseTaxonomyCsv,
  previewTaxonomyRows,
  type FormattedUpdatePreviewResult,
  type TaxonInputRow,
  type TaxonRowOutcome,
  type TaxonomyOperationResult,
} from "../../api/taxonomy";
import { downloadCsv, errorMessage } from "../../api/common";
import { waitForOperation } from "../../api/tasks";
import { operationResult } from "../../app/backgroundTaskResult";
import { getTaxonomyNameSeparator } from "../../api/settings";
import { Busy, Button, EmptyState, SectionHeader, VirtualList } from "../../shared/ui";
import { VariableVirtualList } from "../../shared/VariableVirtualList";
import { TaxonCard } from "./TaxonCard";
import { useMetadataChange } from "../../shared/metadataChanges";
import { useTaxonSearch } from "./useTaxonSearch";
import { useViewState } from "../../shared/viewState";
import { emitTaxonomyMutation, useTaxonomyMutation } from "./taxonomyMutations";
import { ResizablePanels } from "../../shared/ResizablePanels";
import { moveSuggestionSelection } from "../../shared/suggestionNavigation";
import { SearchSuggestions, suggestionLabel } from "../photos/search/SearchSuggestions";
import { useTaxonSuggestions } from "./useTaxonSuggestions";
import { TaxonomyHierarchyPage } from "./TaxonomyHierarchyPage";
import { FormattedUpdateHelpModal } from "./TaxonomyHelpModal";
import {
  currentTaxonForRoot,
  reconcileSelectedRoot,
  recordHierarchyPosition,
  taxonSearchMatchExplanations,
  type HierarchyPositions,
} from "./hierarchyNavigation";
import type { TaxonNameParts } from "../../api/general";

export function TaxonomySearchView({
  onOpenPhotos,
  onStatus,
  nameParts,
  mutationDisabled = false,
}: {
  onOpenPhotos: (taxonId: number, label: string) => void;
  onStatus: (message: string) => void;
  nameParts: TaxonNameParts;
  mutationDisabled?: boolean;
}) {
  const [query, setQuery] = useViewState("taxonomy-search.query", "");
  const [submittedQuery, setSubmittedQuery] = useViewState("taxonomy-search.submitted-query", query);
  const [selectedRootTaxonId, setSelectedRootTaxonId] = useViewState<number | null>(
    "taxonomy-search.selected-root",
    null,
  );
  const [hierarchyPositions, setHierarchyPositions] = useViewState<HierarchyPositions>(
    "taxonomy-search.hierarchy-positions",
    {},
  );
  const [refreshKey, setRefreshKey] = useState(0);
  const [selectedSuggestionIndex, setSelectedSuggestionIndex] = useState(-1);
  const [suggestionsOpen, setSuggestionsOpen] = useState(false);
  const taxonomySearchBoxRef = useRef<HTMLDivElement>(null);
  const taxonomySearch = useTaxonSearch(submittedQuery, {
    debounceMs: 0,
    stateKey: "taxonomy-search.results",
    refreshKey,
  });
  const taxonomySuggestions = useTaxonSuggestions(query, true);
  useTaxonomyMutation(() => {
    setRefreshKey((current) => current + 1);
  });

  useEffect(() => {
    if (!submittedQuery.trim()) {
      onStatus("Ready");
      return;
    }
    if (taxonomySearch.loading) {
      onStatus("Searching...");
    } else if (!taxonomySearch.error) {
      onStatus(taxonomySearch.results.length === 0
        ? "No results"
        : `${taxonomySearch.results.length} results shown`);
    }
  }, [onStatus, submittedQuery, taxonomySearch.error, taxonomySearch.loading, taxonomySearch.results.length]);

  useEffect(() => {
    setSelectedSuggestionIndex(-1);
  }, [query]);

  useEffect(() => {
    if (selectedSuggestionIndex >= taxonomySuggestions.suggestions.length) {
      setSelectedSuggestionIndex(-1);
    }
  }, [selectedSuggestionIndex, taxonomySuggestions.suggestions.length]);

  useEffect(() => {
    const closeSuggestions = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && !taxonomySearchBoxRef.current?.contains(target)) {
        setSuggestionsOpen(false);
        setSelectedSuggestionIndex(-1);
      }
    };
    document.addEventListener("pointerdown", closeSuggestions);
    return () => document.removeEventListener("pointerdown", closeSuggestions);
  }, []);

  useEffect(() => {
    if (!submittedQuery.trim()) {
      setSelectedRootTaxonId(null);
      return;
    }
    setSelectedRootTaxonId((current) => reconcileSelectedRoot(
      current,
      taxonomySearch.results.map((result) => result.taxon_id),
    ));
  }, [submittedQuery, taxonomySearch.results]);

  function submitTaxonomySearch(value = query) {
    const normalized = value.trim();
    if (!normalized) return;
    setQuery(normalized);
    setSubmittedQuery(normalized);
    setRefreshKey((current) => current + 1);
    setSelectedRootTaxonId(null);
    setHierarchyPositions({});
    setSelectedSuggestionIndex(-1);
    setSuggestionsOpen(false);
  }

  const selectedResult = selectedRootTaxonId === null
    ? null
    : taxonomySearch.results.find((result) => result.taxon_id === selectedRootTaxonId) ?? null;
  const selectedCurrentTaxonId = selectedRootTaxonId === null
    ? null
    : currentTaxonForRoot(selectedRootTaxonId, hierarchyPositions);
  const resultsPane = (
    <aside className="taxonomy-results">
      <VariableVirtualList
        stateKey="taxonomy-search.results-list"
        resetKey={`${submittedQuery}:${refreshKey}`}
        items={taxonomySearch.results}
        estimatedRowHeight={90}
        itemKey={(item) => item.taxon_id}
        renderItem={(item) => (
          <TaxonCard
            taxon={item}
            active={selectedRootTaxonId === item.taxon_id}
            matchExplanations={taxonSearchMatchExplanations(item)}
            onClick={() => setSelectedRootTaxonId(item.taxon_id)}
          />
        )}
      />
      {taxonomySearch.loading ? (
        <div className="taxonomy-results-loading" role="status"><Busy label="Searching..." /></div>
      ) : null}
    </aside>
  );
  const recordsPane = (
    <main className="taxonomy-records">
      {taxonomySearch.error ? <EmptyState title="Taxonomy unavailable" detail={taxonomySearch.error} /> : taxonomySearch.loading && selectedResult === null ? (
        <div className="taxonomy-search-loading" role="status" aria-live="polite"><Busy label="Searching taxonomy..." /></div>
      ) : !submittedQuery.trim() ? (
        <EmptyState icon={Search} title="Search taxonomy" detail="Results include accepted names and aliases." />
      ) : selectedResult === null || selectedCurrentTaxonId === null ? (
        <EmptyState icon={Search} title="No taxonomy results" detail={`No taxa matched "${submittedQuery}".`} />
      ) : (
        <TaxonomyHierarchyPage
          key={selectedRootTaxonId}
          initialTaxonId={selectedCurrentTaxonId}
          onTaxonChange={(currentTaxonId) => setHierarchyPositions((current) => (
            recordHierarchyPosition(current, selectedResult.taxon_id, currentTaxonId)
          ))}
          onOpenPhotos={onOpenPhotos}
          nameParts={nameParts}
          mutationDisabled={mutationDisabled}
        />
      )}
    </main>
  );

  return (
    <div className="taxonomy-search-view">
      <header className="workbench-toolbar">
          <div
            className="taxonomy-search-combobox"
            ref={taxonomySearchBoxRef}
            onFocusCapture={() => {
              if (query.trim()) setSuggestionsOpen(true);
            }}
          >
            <label className="search-field taxonomy-search-field">
              <Search size={14} />
              <input
                role="combobox"
                aria-autocomplete="list"
                aria-controls="taxonomy-search-suggestions-listbox"
                aria-expanded={suggestionsOpen && (
                  taxonomySuggestions.loading || taxonomySuggestions.suggestions.length > 0
                )}
                aria-activedescendant={selectedSuggestionIndex >= 0 ? `taxonomy-search-suggestions-option-${selectedSuggestionIndex}` : undefined}
                autoFocus
                autoCapitalize="none"
                autoComplete="off"
                autoCorrect="off"
                spellCheck={false}
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                  setSuggestionsOpen(true);
                }}
                onKeyDown={(event) => {
                  if (event.nativeEvent.isComposing) return;
                  const suggestions = taxonomySuggestions.suggestions;
                  if (suggestions.length > 0 && event.key === "ArrowDown") {
                    event.preventDefault();
                    setSuggestionsOpen(true);
                    setSelectedSuggestionIndex((current) => moveSuggestionSelection(current, suggestions.length, 1));
                    return;
                  }
                  if (suggestions.length > 0 && event.key === "ArrowUp") {
                    event.preventDefault();
                    setSelectedSuggestionIndex((current) => moveSuggestionSelection(current, suggestions.length, -1));
                    return;
                  }
                  if (event.key === "Enter") {
                    event.preventDefault();
                    const suggestion = suggestions[selectedSuggestionIndex];
                    submitTaxonomySearch(suggestion ? suggestionLabel(suggestion, query) : query);
                    return;
                  }
                  if (event.key === "Escape") {
                    setSuggestionsOpen(false);
                    setSelectedSuggestionIndex(-1);
                  }
                }}
                placeholder="Search scientific, Chinese, or English names"
              />
            </label>
            {suggestionsOpen && query.trim() && (
              <SearchSuggestions
                idPrefix="taxonomy-search-suggestions"
                loading={taxonomySuggestions.loading}
                suggestions={taxonomySuggestions.suggestions}
                selectedIndex={selectedSuggestionIndex}
                onHover={setSelectedSuggestionIndex}
                onSelect={(suggestion) => submitTaxonomySearch(suggestionLabel(suggestion, query))}
              />
            )}
          </div>
      </header>
      <ResizablePanels
        className="taxonomy-columns"
        initialRatio={0.28}
        minFirst={220}
        minSecond={360}
        separatorLabel="Resize taxonomy results and details"
        stateKey="taxonomy-search.columns"
        first={resultsPane}
        second={recordsPane}
      />
    </div>
  );
}

const inputFields: Array<keyof TaxonInputRow> = [
  "kingdom", "order", "family", "genus", "species", "authority_year", "synonyms",
  "zh_name", "zh_alias", "en_name", "en_alias", "geological_range", "source",
];

type FormattedBusy = "" | "import" | "template" | "preview" | "apply";

export function FormattedUpdateView({
  onStatus,
  taskOwnerId,
  mutationDisabled = false,
}: {
  onStatus: (message: string) => void;
  taskOwnerId: string;
  mutationDisabled?: boolean;
}) {
  const [rows, setRows] = useState<TaxonInputRow[]>([{ species: "" }]);
  const [outcomes, setOutcomes] = useState<TaxonRowOutcome[]>([]);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState<FormattedBusy>("");
  const [separator, setSeparator] = useState(";");
  const [previewId, setPreviewId] = useState<string | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const previewIdRef = useRef<string | null>(null);

  useEffect(() => {
    getTaxonomyNameSeparator()
      .then(setSeparator)
      .catch((nextError) => report(errorMessage(nextError)));
  }, []);
  useMetadataChange((change) => {
    if (change.key === "taxonomy_name_separator") setSeparator(change.value);
    invalidatePreview();
    if (change.key === "csv_delimiter") {
      report("Previous preview cleared because the CSV delimiter changed.");
    }
  });
  useTaxonomyMutation((mutation) => {
    if (mutation.kind !== "replacement" && previewIdRef.current === null) return;
    invalidatePreview();
    report(mutation.kind === "replacement"
      ? "Previous preview cleared because the taxonomy database was replaced."
      : "Previous preview cleared because the taxonomy changed.");
  });

  function report(nextMessage: string) {
    setMessage(nextMessage);
    onStatus(nextMessage);
  }

  function setCurrentPreview(nextPreviewId: string | null) {
    previewIdRef.current = nextPreviewId;
    setPreviewId(nextPreviewId);
  }

  function invalidatePreview() {
    setCurrentPreview(null);
    setOutcomes([]);
    setMessage("");
  }

  async function importFile(file: File) {
    setBusy("import");
    invalidatePreview();
    try {
      setRows(await parseTaxonomyCsv(await file.text()));
      setOutcomes([]);
      report("CSV imported.");
    } catch (nextError) {
      report(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  async function downloadTemplate() {
    setBusy("template");
    setMessage("");
    try {
      downloadCsv("taxonomy-template.csv", await getTaxonomyTemplate());
      report("Template downloaded.");
    } catch (nextError) {
      report(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  function updateRow(index: number, field: keyof TaxonInputRow, value: string) {
    invalidatePreview();
    setRows((current) => current.map((row, rowIndex) => rowIndex === index ? {
      ...row,
      [field]: ["synonyms", "zh_alias", "en_alias"].includes(field)
        ? value === "" ? [] : value.split(separator)
        : value || null,
    } : row));
  }

  function deleteRow(index: number) {
    if (index === 0) return;
    invalidatePreview();
    setRows((current) => current.filter((_, rowIndex) => rowIndex !== index));
  }

  async function preview() {
    setBusy("preview");
    setMessage("");
    setCurrentPreview(null);
    try {
      const started = await previewTaxonomyRows(rows, taskOwnerId);
      const completed = started.task_id && ["queued", "running"].includes(started.state)
        ? await waitForOperation(started.task_id)
        : started;
      const result = operationResult<FormattedUpdatePreviewResult>(completed, started.task_id);
      setOutcomes(result.rows);
      setCurrentPreview(result.preview_id);
      report(`${result.rows.length} rows previewed`);
    } catch (nextError) {
      report(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  async function apply() {
    if (previewId === null) return;
    const currentPreviewId = previewId;
    setBusy("apply");
    setMessage("");
    setCurrentPreview(null);
    try {
      const started = await applyTaxonomyRows(currentPreviewId, taskOwnerId);
      const completed = started.task_id && ["queued", "running"].includes(started.state)
        ? await waitForOperation(started.task_id)
        : started;
      const result = operationResult<TaxonomyOperationResult>(completed, started.task_id);
      setOutcomes(result.rows);
      report(`${result.succeeded_rows} succeeded, ${result.failed_rows} failed`);
      emitTaxonomyMutation();
    } catch (nextError) {
      report(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  return (
    <div className="formatted-view">
      <SectionHeader title="Formatted update" detail="UTF-8 CSV input or direct table editing." actions={
        <>
          <Button onClick={() => setHelpOpen(true)}><CircleQuestionMark size={13} />Help</Button>
          <label className={`button button-secondary file-button${busy ? " disabled" : ""}`}><FileUp size={13} />{busy === "import" ? "Importing..." : "Upload CSV"}<input disabled={Boolean(busy)} type="file" accept=".csv,text/csv" onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) void importFile(file);
          }} /></label>
          <Button disabled={Boolean(busy)} onClick={() => void downloadTemplate()}><Download size={13} />{busy === "template" ? "Loading..." : "Download Template"}</Button>
          <Button disabled={Boolean(busy)} onClick={() => void preview()}><Eye size={13} />{busy === "preview" ? "Previewing..." : "Preview"}</Button>
          <Button variant="primary" disabled={Boolean(busy) || mutationDisabled || previewId === null} onClick={() => void apply()}><Play size={13} />{busy === "apply" ? "Applying..." : "Apply"}</Button>
        </>
      } />
      {helpOpen && <FormattedUpdateHelpModal onClose={() => setHelpOpen(false)} />}
      <ResizablePanels
        className="formatted-split"
        direction="vertical"
        initialRatio={0.52}
        minFirst={190}
        minSecond={150}
        separatorLabel="Resize formatted input and result log"
        stateKey="formatted-update.rows"
        first={(<div className="input-table">
        <div className="input-table-head"><span>#</span>{inputFields.map((field) => <span key={field}>{field}</span>)}<span /></div>
        <VirtualList
          items={rows}
          rowHeight={38}
          itemKey={(_, index) => index}
          renderItem={(row, index) => (
            <div className="input-table-row">
              <span>{index + 1}</span>
              {inputFields.map((field) => <input disabled={Boolean(busy)} key={field} value={Array.isArray(row[field]) ? (row[field] as string[]).join(separator) : String(row[field] ?? "")} onChange={(event) => updateRow(index, field, event.target.value)} />)}
              <button
                className="input-row-delete"
                type="button"
                aria-label={`Delete row ${index + 1}`}
                title={index === 0 ? "The first row cannot be deleted" : "Delete row"}
                disabled={Boolean(busy) || index === 0}
                onClick={() => deleteRow(index)}
              ><Trash2 size={13} /></button>
            </div>
          )}
        />
        <Button className="table-add-row" variant="ghost" disabled={Boolean(busy)} onClick={() => {
          invalidatePreview();
          setRows((current) => [...current, {}]);
        }}>+ Add row</Button>
        </div>)}
        second={(<div className="formatted-log">
        <SectionHeader title="Result log" detail={message || "Preview is required before apply"} />
        <VirtualList
          items={outcomes}
          rowHeight={64}
          itemKey={(item) => item.row_number}
          renderItem={(item) => (
            <div className="log-row">
              <b>{item.row_number}</b>
              <div><strong>{item.operation_types.join(" + ")}</strong><span>{item.message}</span></div>
              <code>{item.changes.map((change) => `${change.field}: ${change.old_value ?? "-"} -> ${change.new_value ?? "-"}`).join(" | ")}</code>
            </div>
          )}
        />
        </div>)}
      />
    </div>
  );
}
