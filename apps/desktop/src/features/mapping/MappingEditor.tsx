import { Link, Search, Sparkles, Unlink } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import {
  clearPhotoMapping,
  getPhotoMappingDetail,
  setPhotoMapping,
  type PhotoMappingDetail,
} from "../../api/mapping";
import type { Photo } from "../../api/photos";
import {
  getTaxonDetail,
  type TaxonDetail,
} from "../../api/taxonomy";
import { errorMessage } from "../../api/common";
import { Busy, Button, EmptyState } from "../../shared/ui";
import { VariableVirtualList } from "../../shared/VariableVirtualList";
import { PhotoStage } from "../photos/PhotoMedia";
import { TaxonCard } from "../taxonomy/TaxonCard";
import { taxonMatchExplanations } from "../taxonomy/taxonMatchExplanation";
import { MappingBadge } from "./MappingBadge";
import { emitPhotoMutation, usePhotoMutation } from "../photos/photoMutations";
import { useTaxonSearch } from "../taxonomy/useTaxonSearch";
import { useViewState } from "../../shared/viewState";
import { ResizablePanels } from "../../shared/ResizablePanels";
import { PhotoPaneHeader } from "../photos/PhotoPaneHeader";
import { usePublishedPhotoTaxonSummary, type PhotoTaxonDisplayState } from "../photos/photoTaxonSummary";
import { usePhotoInteraction, type PhotoOpenHandlers } from "../photos/PhotoInteraction";

const ignorePhotoTaxonDisplayState = (_state: PhotoTaxonDisplayState | null) => {};

function StandaloneMappingPhotoPane({
  photo,
  handlers,
  onStatus,
}: {
  photo: Photo;
  handlers: PhotoOpenHandlers;
  onStatus: (message: string) => void;
}) {
  const interaction = usePhotoInteraction({
    photos: [photo],
    handlers,
    selectFirst: false,
    stateKey: "mapping-editor.interaction",
    onStatus,
  });

  return (
    <>
      <div className="editor-photo-column">
        <div className="photo-pane-heading">
          <PhotoPaneHeader photo={photo} />
        </div>
        <PhotoStage photo={photo} onContextMenu={interaction.openContextMenu} />
      </div>
      {interaction.contextMenu}
    </>
  );
}

export function MappingEditor({
  photo,
  embedded = false,
  active = false,
  onPhotoTaxonDisplayState = ignorePhotoTaxonDisplayState,
  handlers,
  onStatus,
  refreshKey = 0,
}: {
  photo: Photo;
  embedded?: boolean;
  active?: boolean;
  onPhotoTaxonDisplayState?: (state: PhotoTaxonDisplayState | null) => void;
  handlers: PhotoOpenHandlers;
  onStatus: (message: string) => void;
  refreshKey?: number;
}) {
  const [match, setMatch] = useViewState<PhotoMappingDetail | null>("mapping-editor.match", null);
  const [mappedTaxon, setMappedTaxon] = useViewState<TaxonDetail | null>("mapping-editor.taxon", null);
  const [query, setQuery] = useViewState("mapping-editor.query", "");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [mappingRefresh, setMappingRefresh] = useState(0);
  const taxonomySearch = useTaxonSearch(query, {
    limit: 60,
    stateKey: "mapping-editor.search",
  });
  usePhotoMutation((mutation) => {
    if (
      mutation.kind === "mapping"
      && (mutation.photoId === null || mutation.photoId === photo.photo_id)
    ) {
      setMappingRefresh((current) => current + 1);
    }
  });
  usePublishedPhotoTaxonSummary({
    photoId: embedded ? null : photo.photo_id,
    active: active && !embedded,
    onChange: onPhotoTaxonDisplayState,
  });

  const reload = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const nextMatch = await getPhotoMappingDetail(photo.photo_id);
      setMatch(nextMatch);
      if (nextMatch.mapping.status === "matched" && nextMatch.mapping.taxon_id !== null) {
        setMappedTaxon(await getTaxonDetail(nextMatch.mapping.taxon_id));
      } else {
        setMappedTaxon(null);
      }
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setLoading(false);
    }
  }, [photo.photo_id, refreshKey, mappingRefresh]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function mutate(label: string, action: () => Promise<unknown>) {
    setBusy(label);
    setError("");
    try {
      await action();
      await reload();
      emitPhotoMutation({ photoId: photo.photo_id, kind: "mapping" });
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  const photoPane = <StandaloneMappingPhotoPane photo={photo} handlers={handlers} onStatus={onStatus} />;
  const currentTaxon = mappedTaxon ? {
    taxon_id: mappedTaxon.taxon_id,
    rank: mappedTaxon.rank,
    names: {
      sci_name: mappedTaxon.names.sci_name?.name ?? null,
      zh_name: mappedTaxon.names.zh_name?.name ?? null,
      en_name: mappedTaxon.names.en_name?.name ?? null,
    },
  } : null;
  const currentPane = (
    <section className="mapping-current">
      <header>
        <strong>Current mapping</strong>
        {match && <MappingBadge status={match.mapping.status} />}
      </header>
      {loading ? (
        <Busy label="Loading mapping" />
      ) : match?.mapping.status === "matched" && currentTaxon ? (
        <TaxonCard
          compact
          taxon={currentTaxon}
          matchExplanations={taxonMatchExplanations(match.matched_names)}
          onClick={() => handlers.openTaxon(currentTaxon.taxon_id)}
          actions={(
            <Button
              size="small"
              disabled={Boolean(busy)}
              onClick={() => void mutate("Unmapping", () => clearPhotoMapping(photo.photo_id))}
            >
              <Unlink size={12} /> {busy === "Unmapping" ? "Unmapping..." : "Unmap"}
            </Button>
          )}
        />
      ) : match?.mapping.status === "ambiguous" ? (
        <VariableVirtualList
          stateKey="mapping-editor.candidates"
          resetKey={`${photo.photo_id}:${refreshKey}:${mappingRefresh}`}
          className="candidate-stack"
          items={match.candidates}
          estimatedRowHeight={90}
          itemKey={(candidate) => candidate.summary.taxon_id}
          renderItem={(candidate) => (
            <TaxonCard
              compact
              taxon={candidate.summary}
              matchExplanations={taxonMatchExplanations(candidate.matched_names)}
              onClick={() => handlers.openTaxon(candidate.summary.taxon_id)}
              actions={
                <Button
                  size="small"
                  disabled={Boolean(busy)}
                  onClick={() => void mutate(`Selecting ${candidate.summary.taxon_id}`, () => setPhotoMapping(photo.photo_id, candidate.summary.taxon_id))}
                >
                  {busy === `Selecting ${candidate.summary.taxon_id}` ? "Selecting..." : "Select"}
                </Button>
              }
            />
          )}
        />
      ) : (
        <EmptyState title="No automatic match" detail="Search below to assign any taxon." icon={Sparkles} />
      )}
    </section>
  );
  const searchPane = (
    <section className="mapping-search">
      <label className="search-field">
        <Search size={14} />
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search taxonomy" />
      </label>
      <VariableVirtualList
        stateKey="mapping-editor.results"
        resetKey={query.trim()}
        className="mapping-search-results"
        items={taxonomySearch.results}
        estimatedRowHeight={90}
        itemKey={(item) => item.taxon_id}
        renderItem={(item) => (
          <TaxonCard
            compact
            taxon={item}
            matchExplanations={taxonMatchExplanations(item.matches)}
            onClick={() => handlers.openTaxon(item.taxon_id)}
            actions={
              <Button size="small" disabled={Boolean(busy)} onClick={() => void mutate(`Mapping ${item.taxon_id}`, () => setPhotoMapping(photo.photo_id, item.taxon_id))}>
                <Link size={12} /> {busy === `Mapping ${item.taxon_id}` ? "Mapping..." : "Map"}
              </Button>
            }
          />
        )}
      />
    </section>
  );
  const mappingPane = (
    <div className="editor-mapping-column">
      <ResizablePanels
        className="editor-mapping-split"
        direction="vertical"
        initialRatio={0.36}
        minFirst={180}
        minSecond={180}
        separatorLabel="Resize current mapping and taxonomy search"
        stateKey={embedded ? "mapping-editor.embedded-sections" : "mapping-editor.sections"}
        first={currentPane}
        second={searchPane}
      />
      {(error || taxonomySearch.error) && <div className="inline-error">{error || taxonomySearch.error}</div>}
    </div>
  );

  if (embedded) return <div className="mapping-editor embedded">{mappingPane}</div>;

  return (
    <ResizablePanels
      className="mapping-editor"
      initialRatio={0.58}
      minFirst={300}
      minSecond={330}
      separatorLabel="Resize mapping photo and controls"
      stateKey="mapping-editor.columns"
      first={photoPane}
      second={mappingPane}
    />
  );
}
