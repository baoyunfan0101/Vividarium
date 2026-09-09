import { ChevronDown, ChevronRight, Images, Trash2 } from "lucide-react";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import {
  deleteTaxon,
  deleteTaxonName,
  getTaxonDetail,
  listTaxonChildren,
  promoteTaxonName,
  saveTaxonNameGroup,
  type SaveTaxonNameGroupInput,
  type TaxonChild,
  type TaxonDetail,
  type TaxonNameDetail,
} from "../../api/taxonomy";
import type { TaxonNameParts } from "../../api/general";
import { errorMessage } from "../../api/common";
import { Busy, Button, EmptyState } from "../../shared/ui";
import { useCursorPage } from "../../shared/useCursorPage";
import { emitTaxonomyMutation, useTaxonomyMutation } from "./taxonomyMutations";
import {
  createHierarchyNavigationState,
  hierarchyNavigationReducer,
} from "./hierarchyNavigation";
import {
  TaxonConfirmationModal,
  type TaxonConfirmation,
} from "./TaxonConfirmationModal";
import { TaxonNameGroupEditor } from "./TaxonNameGroupEditor";
import {
  acceptedTaxonNameGroup,
  canDeleteTaxonName,
  taxonNameGroupLabels,
  type TaxonNameGroupKind,
} from "./taxonEditing";
import { formatTaxonName } from "./taxonNameFormatting";
import { selectionIntersectsElement } from "../../shared/selectableSurface";

type TaxonomyHierarchyPageProps = {
  initialTaxonId: number;
  onTaxonChange?: (taxonId: number, label: string) => void;
  onOpenPhotos: (taxonId: number, label: string) => void;
  nameParts: TaxonNameParts;
  mutationDisabled?: boolean;
};

type NameGroup = {
  kind: TaxonNameGroupKind;
  records: TaxonNameDetail[];
};

type EditingState =
  | { kind: "name-group"; group: TaxonNameGroupKind }
  | TaxonConfirmation
  | null;

export function TaxonomyHierarchyPage({
  initialTaxonId,
  onTaxonChange,
  onOpenPhotos,
  nameParts,
  mutationDisabled = false,
}: TaxonomyHierarchyPageProps) {
  const [navigation, dispatch] = useReducer(
    hierarchyNavigationReducer,
    initialTaxonId,
    createHierarchyNavigationState,
  );
  const [detail, setDetail] = useState<TaxonDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [detailError, setDetailError] = useState("");
  const [editing, setEditing] = useState<EditingState>(null);
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState("");
  const [mutationStatus, setMutationStatus] = useState("");
  const [deletedParent, setDeletedParent] = useState<{ taxonId: number; label: string } | null>(null);
  const detailRequest = useRef(0);
  const onTaxonChangeRef = useRef(onTaxonChange);
  onTaxonChangeRef.current = onTaxonChange;
  const currentTaxonId = navigation.currentTaxonId;
  const children = useCursorPage<TaxonChild, number>({
    params: currentTaxonId,
    resetKey: currentTaxonId,
    enabled: navigation.childrenRequested,
    loadPage: (taxonId, cursor) => listTaxonChildren(taxonId, cursor),
  });

  const loadDetail = useCallback(async (taxonId: number, retainDetail = false) => {
    const request = ++detailRequest.current;
    setLoading(true);
    setDetailError("");
    if (!retainDetail) {
      setDetail(null);
      setDeletedParent(null);
    }
    try {
      const next = await getTaxonDetail(taxonId);
      if (request !== detailRequest.current) return;
      setDetail(next);
      onTaxonChangeRef.current?.(
        next.taxon_id,
        formatTaxonName(detailDisplayNames(next), nameParts, `Taxon ${next.taxon_id}`),
      );
    } catch (nextError) {
      if (request !== detailRequest.current) return;
      setDetailError(errorMessage(nextError));
      setDetail(null);
    } finally {
      if (request === detailRequest.current) setLoading(false);
    }
  }, [nameParts]);

  useEffect(() => {
    dispatch({ type: "navigate", taxonId: initialTaxonId });
  }, [initialTaxonId]);

  useEffect(() => {
    setEditing(null);
    setMutationError("");
    setMutationStatus("");
    void loadDetail(currentTaxonId);
    return () => {
      detailRequest.current += 1;
    };
  }, [currentTaxonId, loadDetail]);

  useTaxonomyMutation((mutation) => {
    if (mutation.kind === "update" && mutation.deletedTaxonId === currentTaxonId) {
      const parent = detail?.breadcrumb[detail.breadcrumb.length - 1];
      detailRequest.current += 1;
      setDeletedParent(parent ? {
        taxonId: parent.taxon_id,
        label: formatTaxonName(parent.names, nameParts, `Taxon ${parent.taxon_id}`),
      } : null);
      setDetail(null);
      setDetailError("This taxon was deleted.");
      setLoading(false);
      dispatch({ type: "reset", taxonId: currentTaxonId });
      children.updateItems([]);
      return;
    }
    void loadDetail(currentTaxonId, true);
    if (navigation.childrenRequested) void children.reload();
  });

  function navigateTo(taxonId: number, label: string) {
    setEditing(null);
    setMutationError("");
    setMutationStatus("");
    dispatch({ type: "navigate", taxonId });
    onTaxonChange?.(taxonId, label);
  }

  function openEditing(next: Exclude<EditingState, null>) {
    setMutationError("");
    setMutationStatus("");
    setEditing(next);
  }

  async function applyNameGroupSave(input: SaveTaxonNameGroupInput) {
    setMutationBusy(true);
    setMutationError("");
    try {
      await yieldToPaint();
      await saveTaxonNameGroup(input);
      setEditing(null);
      setMutationStatus(`Saved ${taxonNameGroupLabels[input.name_type]}.`);
      emitTaxonomyMutation({ kind: "update", taxonId: input.taxon_id });
    } catch (nextError) {
      setMutationError(errorMessage(nextError));
    } finally {
      setMutationBusy(false);
    }
  }

  async function applyConfirmation() {
    if (!editing || editing.kind === "name-group" || detail === null) return;
    setMutationBusy(true);
    setMutationError("");
    try {
      await yieldToPaint();
      if (editing.kind === "promote-name") {
        await promoteTaxonName({ taxon_id: detail.taxon_id, name_id: editing.record.name_id });
        setMutationStatus(`"${editing.record.name}" is now accepted.`);
        emitTaxonomyMutation({ kind: "update", taxonId: detail.taxon_id });
      } else if (editing.kind === "delete-name") {
        await deleteTaxonName({ taxon_id: detail.taxon_id, name_id: editing.record.name_id });
        setMutationStatus(`Deleted "${editing.record.name}".`);
        emitTaxonomyMutation({ kind: "update", taxonId: detail.taxon_id });
      } else {
        await deleteTaxon(detail.taxon_id);
        setMutationStatus(`Deleted ${formatTaxonName(detailDisplayNames(detail), nameParts, `Taxon ${detail.taxon_id}`)}.`);
        emitTaxonomyMutation({
          kind: "update",
          taxonId: detail.taxon_id,
          deletedTaxonId: detail.taxon_id,
        });
      }
      setEditing(null);
    } catch (nextError) {
      setMutationError(errorMessage(nextError));
    } finally {
      setMutationBusy(false);
    }
  }

  if (loading && detail === null) {
    return <div className="taxonomy-hierarchy-status"><Busy label="Loading taxon..." /></div>;
  }
  if (detail === null) {
    return (
      <div className="taxonomy-hierarchy-status">
        <EmptyState
          title={detailError === "This taxon was deleted." ? "Taxon deleted" : "Taxon unavailable"}
          detail={detailError || "The taxon could not be loaded."}
          action={deletedParent ? (
            <Button onClick={() => navigateTo(deletedParent.taxonId, deletedParent.label)}>
              Open parent taxon
            </Button>
          ) : undefined}
        />
      </div>
    );
  }

  const currentLabel = formatTaxonName(
    detailDisplayNames(detail),
    nameParts,
    `Taxon ${detail.taxon_id}`,
  );
  const nameGroups: NameGroup[] = [
    { kind: "sci_name", records: detail.names.sci_name ? [detail.names.sci_name] : [] },
    { kind: "synonym", records: detail.names.synonyms },
    { kind: "zh_name", records: detail.names.zh_name ? [detail.names.zh_name] : [] },
    { kind: "zh_alias", records: detail.names.zh_aliases },
    { kind: "en_name", records: detail.names.en_name ? [detail.names.en_name] : [] },
    { kind: "en_alias", records: detail.names.en_aliases },
  ];
  const primaryExists: Record<TaxonNameGroupKind, boolean> = {
    sci_name: detail.names.sci_name !== null,
    synonym: detail.names.sci_name !== null,
    zh_name: detail.names.zh_name !== null,
    zh_alias: detail.names.zh_name !== null,
    en_name: detail.names.en_name !== null,
    en_alias: detail.names.en_name !== null,
  };
  const deleteAllowed: Record<TaxonNameGroupKind, boolean> = {
    sci_name: canDeleteTaxonName("sci_name", false),
    synonym: canDeleteTaxonName("synonym", false),
    zh_name: canDeleteTaxonName("zh_name", detail.names.zh_aliases.length > 0),
    zh_alias: canDeleteTaxonName("zh_alias", false),
    en_name: canDeleteTaxonName("en_name", detail.names.en_aliases.length > 0),
    en_alias: canDeleteTaxonName("en_alias", false),
  };

  return (
    <>
      <main className="taxonomy-hierarchy-page">
        <div className="taxonomy-hierarchy-scroll">
        <nav className="taxonomy-hierarchy-breadcrumb" aria-label="Taxonomy breadcrumb">
          {detail.breadcrumb.map((item) => (
            <span key={item.taxon_id}>
              <button type="button" onClick={() => navigateTo(
                item.taxon_id,
                formatTaxonName(item.names, nameParts, `Taxon ${item.taxon_id}`),
              )}>
                {formatTaxonName(item.names, nameParts, `Taxon ${item.taxon_id}`)}
              </button>
              <ChevronRight size={12} />
            </span>
          ))}
          <strong>{currentLabel}</strong>
        </nav>

        <header className="taxonomy-hierarchy-heading">
          <div>
            <span className="taxon-rank">{detail.rank}</span>
            <h2>{currentLabel}</h2>
            <small>Taxon {detail.taxon_id}</small>
          </div>
          <div className="taxonomy-hierarchy-actions">
            <Button onClick={() => onOpenPhotos(detail.taxon_id, currentLabel)}>
              <Images size={14} /> Photos
            </Button>
            <Button
              className="taxonomy-danger-button"
              disabled={mutationDisabled || mutationBusy || editing !== null}
              onClick={() => openEditing({ kind: "delete-taxon" })}
            >
              <Trash2 size={14} /> Delete taxon
            </Button>
          </div>
        </header>

        {mutationStatus ? (
          <div className="taxonomy-mutation-status" role="status" aria-live="polite">
            {mutationStatus}
          </div>
        ) : null}

        {loading ? (
          <div className="taxonomy-hierarchy-refreshing" role="status" aria-live="polite">
            <Busy label="Refreshing taxon..." />
          </div>
        ) : null}

        <dl className="taxonomy-detail-meta">
          <div><dt>Rank</dt><dd>{detail.rank}</dd></div>
          <div><dt>Taxon ID</dt><dd>{detail.taxon_id}</dd></div>
          <div><dt>Geological range</dt><dd>{detail.geological_range ?? "-"}</dd></div>
        </dl>

        <section className="taxonomy-name-groups" aria-label="Taxon names">
          {nameGroups.map((group) => (
            <TaxonNameGroupEditor
              key={group.kind}
              taxonId={detail.taxon_id}
              kind={group.kind}
              records={group.records}
              primaryExists={primaryExists[acceptedTaxonNameGroup(group.kind)]}
              deleteAllowed={deleteAllowed[group.kind]}
              active={editing?.kind === "name-group" && editing.group === group.kind}
              busy={mutationBusy}
              error={editing?.kind === "name-group" && editing.group === group.kind ? mutationError : ""}
              disabled={mutationDisabled || mutationBusy || editing !== null}
              onStartEditing={() => openEditing({ kind: "name-group", group: group.kind })}
              onCancelEditing={() => setEditing(null)}
              onSave={(input) => void applyNameGroupSave(input)}
              onPromote={(record) => openEditing({ kind: "promote-name", group: group.kind, record })}
              onDelete={(record) => openEditing({ kind: "delete-name", group: group.kind, record })}
            />
          ))}
        </section>

        <section className="taxonomy-children">
          <button
            className="taxonomy-children-toggle"
            type="button"
            aria-expanded={navigation.childrenExpanded}
            onClick={() => dispatch({ type: "toggle-children" })}
          >
            {navigation.childrenExpanded ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
            <strong>Children</strong>
          </button>
          {navigation.childrenExpanded && (
            <div className="taxonomy-child-list">
              {children.loading && children.items.length === 0 ? <Busy label="Loading children..." /> : null}
              {children.error ? <div className="inline-error">{children.error}</div> : null}
              {!children.loading && !children.error && children.items.length === 0 ? (
                <span className="taxonomy-children-empty">No direct children</span>
              ) : null}
              {children.items.map((child) => (
                <div
                  className="taxonomy-child-button selectable-content"
                  role="button"
                  tabIndex={0}
                  key={child.taxon_id}
                  onClick={(event) => {
                    if (selectionIntersectsElement(event.currentTarget)) return;
                    navigateTo(
                      child.taxon_id,
                      formatTaxonName(child.names, nameParts, `Taxon ${child.taxon_id}`),
                    );
                  }}
                  onKeyDown={(event) => {
                    if (event.key !== "Enter" && event.key !== " ") return;
                    event.preventDefault();
                    navigateTo(
                      child.taxon_id,
                      formatTaxonName(child.names, nameParts, `Taxon ${child.taxon_id}`),
                    );
                  }}
                >
                  <span className="taxon-rank">{child.rank}</span>
                  <strong>{formatTaxonName(child.names, nameParts, `Taxon ${child.taxon_id}`)}</strong>
                  <span>Taxon {child.taxon_id}</span>
                  <ChevronRight size={14} />
                </div>
              ))}
              {children.hasMore ? (
                <Button disabled={children.loading} onClick={() => void children.loadMore()}>
                  {children.loading ? "Loading..." : "Load more"}
                </Button>
              ) : null}
            </div>
          )}
        </section>
        </div>
      </main>
      {editing && editing.kind !== "name-group" ? (
        <TaxonConfirmationModal
          confirmation={editing}
          taxonLabel={currentLabel}
          busy={mutationBusy}
          error={mutationError}
          onClose={() => setEditing(null)}
          onConfirm={() => void applyConfirmation()}
        />
      ) : null}
    </>
  );
}

function yieldToPaint(): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, 0));
}

function detailDisplayNames(detail: TaxonDetail) {
  return {
    sci_name: detail.names.sci_name?.name ?? null,
    zh_name: detail.names.zh_name?.name ?? null,
    en_name: detail.names.en_name?.name ?? null,
  };
}
