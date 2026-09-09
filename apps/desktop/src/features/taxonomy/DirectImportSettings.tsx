import { DatabaseArrowUp, LoaderCircle } from "lucide-react";
import { useState } from "react";
import {
  applyDirectImport,
  inspectDirectImportDatabase,
  type DirectImportDatabase,
} from "../../api/directImport";
import { errorMessage } from "../../api/common";
import { selectSqliteDatabase } from "../../api/dialogs";
import { getDatabaseLocations } from "../../api/storage";
import { waitForOperation, type OperationState } from "../../api/tasks";
import { operationResult } from "../../app/backgroundTaskResult";
import type { TaxonomyImportResult } from "../../api/taxonomyImport";
import { backgroundStageLabel } from "../../app/backgroundPresentation";
import { Button, SectionHeader } from "../../shared/ui";
import { formatTaxonomyImportApplyMessage } from "./taxonomyImportMessages";
import { emitTaxonomyMutation } from "./taxonomyMutations";
import { SqlSourceSchemaObjects } from "./SqlInputList";

export function DirectImportSettings({
  active = true,
  onApplied,
  taskOwnerId,
}: {
  active?: boolean;
  onApplied?: () => void;
  taskOwnerId: string;
}) {
  const [database, setDatabase] = useState<DirectImportDatabase | null>(null);
  const [operation, setOperation] = useState<OperationState | null>(null);
  const [busy, setBusy] = useState<"" | "inspect" | "apply">("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  async function chooseDatabase() {
    setMessage("");
    setError("");
    try {
      const locations = await getDatabaseLocations();
      const sourcePath = await selectSqliteDatabase(locations.default_taxonomy_directory);
      if (!sourcePath) return;
      setBusy("inspect");
      const started = await inspectDirectImportDatabase(sourcePath, taskOwnerId);
      const completed = started.task_id && ["queued", "running"].includes(started.state)
        ? await waitForOperation(started.task_id)
        : started;
      setDatabase(operationResult<DirectImportDatabase>(completed, started.task_id));
    } catch (nextError) {
      setDatabase(null);
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  async function confirmImport() {
    if (!database) return;
    setMessage("");
    setError("");
    setBusy("apply");
    try {
      const started = await applyDirectImport(database.source_path, taskOwnerId);
      setOperation(started);
      const completed = started.task_id
        ? await waitForOperation(started.task_id, setOperation)
        : started;
      if (completed.error) throw new Error(completed.error);
      const result = completed.result as TaxonomyImportResult | null;
      if (!result) throw new Error("Direct import completed without a replacement result");

      onApplied?.();
      emitTaxonomyMutation({ kind: "replacement" });
      setDatabase(null);
      setMessage(formatTaxonomyImportApplyMessage(result.warnings));
    } catch (nextError) {
      setMessage("");
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
      setOperation(null);
    }
  }

  const progressMessage = operation ? backgroundStageLabel(operation) : "Applying direct import";

  const sourceName = database?.source_path.split(/[\\/]/).pop() ?? "SQLite database";

  return (
    <div aria-hidden={!active} className={`settings-section direct-import-settings${active ? "" : " inactive"}`}>
      <SectionHeader
        title="Direct Import"
        detail="Replace the current taxonomy with a ready-to-use SQLite database."
        actions={(
          <>
            <Button disabled={Boolean(busy)} onClick={() => void chooseDatabase()}>
              <DatabaseArrowUp size={13} />{busy === "inspect" ? "Inspecting..." : database ? "Choose another" : "Import"}
            </Button>
            <Button variant="primary" disabled={Boolean(busy) || !database} onClick={() => void confirmImport()}>
              <DatabaseArrowUp size={13} />{busy === "apply" ? "Importing..." : "Confirm import"}
            </Button>
          </>
        )}
      />
      {database ? (
        <aside className="sql-sources direct-import-source-list">
          <header className="sql-input-actions"><strong>Input sources</strong></header>
          <section className="sql-source-card">
            <header><span><b>{sourceName}</b><i>SQLITE</i></span></header>
            <code title={database.source_path}>{database.source_path}</code>
            <small className="available">Validated and ready to import</small>
            <SqlSourceSchemaObjects schema={database.schema} />
          </section>
        </aside>
      ) : (
        <div className="direct-import-description">
          <strong>Import a taxonomy database directly</strong>
          <p>Select a SQLite database to inspect its path and tables before confirming the replacement.</p>
          <p>The database must contain valid taxa and taxon_names tables using the current schema.</p>
        </div>
      )}
      {busy === "apply" ? (
        <div className="direct-import-progress" role="status" aria-live="polite">
          <LoaderCircle className="spin" size={15} />
          <strong>{progressMessage}</strong>
        </div>
      ) : error ? (
        <div className="inline-error direct-import-status" role="alert">{error}</div>
      ) : message ? (
        <div className="editor-message direct-import-status">{message}</div>
      ) : null}
    </div>
  );
}
