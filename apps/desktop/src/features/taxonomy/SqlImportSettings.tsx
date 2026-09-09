import { CircleQuestionMark, LoaderCircle, Send, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import {
  addSqlImportInput,
  applySqlImport,
  getSqlImportSql,
  listSqlImportDatabaseSchemas,
  listSqlImportInputs,
  listSqlImportStagingSchemas,
  removeSqlImportInput,
  startSqlImportValidation,
  type SqlImportValidationResult,
  type ValidateSqlImportResult,
} from "../../api/sqlImport";
import type { TaxonomyImportResult } from "../../api/taxonomyImport";
import type { PersistentSqlInput, SqlSourceSchema } from "../../api/customSql";
import { errorMessage } from "../../api/common";
import { waitForOperation, type OperationState } from "../../api/tasks";
import { CodeEditor } from "../../shared/CodeEditor";
import { ResizablePanels } from "../../shared/ResizablePanels";
import { Button, Modal, SectionHeader, VirtualList } from "../../shared/ui";
import { SqlInputList } from "./SqlInputList";
import { SqlEnumHelpModal } from "./TaxonomyHelpModal";
import { emitTaxonomyMutation } from "./taxonomyMutations";
import { formatTaxonomyImportApplyMessage } from "./taxonomyImportMessages";
import {
  SQL_IMPORT_VALIDATION_ISSUE_ROW_HEIGHT,
  sqlImportValidationIssueRow,
} from "./sqlImportValidation";
import { resolveSqlWorkbenchLoads } from "./sqlWorkbenchLoading";
import { sqlOperationProgress } from "./sqlOperationProgress";

export function SqlImportSettings({
  active = true,
  onApplied,
  taskOwnerId,
}: {
  active?: boolean;
  onApplied?: () => void;
  taskOwnerId: string;
}) {
  const [inputs, setInputs] = useState<PersistentSqlInput[]>([]);
  const [databaseSchemas, setDatabaseSchemas] = useState<SqlSourceSchema[]>([]);
  const [stagingSchemas, setStagingSchemas] = useState<SqlSourceSchema[]>([]);
  const [sql, setSql] = useState("");
  const [validation, setValidation] = useState<SqlImportValidationResult | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [loadingWorkbench, setLoadingWorkbench] = useState(true);
  const [helpOpen, setHelpOpen] = useState(false);
  const [operation, setOperation] = useState<OperationState | null>(null);

  useEffect(() => {
    void Promise.allSettled([
      getSqlImportSql(),
      listSqlImportInputs(),
      listSqlImportDatabaseSchemas(),
      listSqlImportStagingSchemas(),
    ])
      .then(([sqlResult, inputsResult, schemasResult, stagingResult]) => {
        const loaded = resolveSqlWorkbenchLoads(sqlResult, inputsResult);
        if (loaded.sql !== undefined) setSql(loaded.sql);
        if (loaded.inputs !== undefined) setInputs(loaded.inputs);
        if (schemasResult.status === "fulfilled") setDatabaseSchemas(schemasResult.value);
        if (stagingResult.status === "fulfilled") setStagingSchemas(stagingResult.value);
        const schemasError = schemasResult.status === "rejected" ? errorMessage(schemasResult.reason) : "";
        const stagingError = stagingResult.status === "rejected" ? errorMessage(stagingResult.reason) : "";
        setError([loaded.error, schemasError, stagingError].filter(Boolean).join(" "));
      })
      .finally(() => setLoadingWorkbench(false));
  }, []);

  async function addInput(kind: "csv" | "sqlite", alias: string, path: string) {
    setBusy("Adding data source");
    setMessage("");
    setError("");
    try {
      const result = await addSqlImportInput(kind, alias, path);
      setInputs(result.inputs);
      setStagingSchemas([]);
      setValidation(null);
      setMessage(result.warnings.length > 0 ? result.warnings.join(" ") : "Data source added.");
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  async function validate() {
    setBusy("Validating SQL import");
    setMessage("");
    setError("");
    setValidation(null);
    setOperation(null);
    try {
      const started = await startSqlImportValidation(sql, taskOwnerId);
      setOperation(started);
      const completed = started.task_id
        ? await waitForOperation(started.task_id, setOperation)
        : started;
      if (completed.error) throw new Error(completed.error);
      const result = completed.result as ValidateSqlImportResult | null;
      if (!result) throw new Error("SQL import validation completed without a result");
      setValidation(result.validation);
      try {
        setStagingSchemas(await listSqlImportStagingSchemas());
      } catch (schemasError) {
        setError(errorMessage(schemasError));
      }
      const saveStatus = result.execution.script_saved ? "SQL saved." : "SQL was not saved.";
      setMessage([
        `${result.execution.statements_executed} statements executed successfully.`,
        saveStatus,
        result.can_apply
          ? "Candidate is valid and ready to apply."
          : `Validation found ${result.validation.total_error_count} errors.`,
        ...result.warnings,
      ].join(" "));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
      setOperation(null);
    }
  }

  async function apply() {
    setBusy("Applying SQL import");
    setMessage("");
    setError("");
    try {
      const operation = await applySqlImport(taskOwnerId);
      const completed = operation.task_id
        ? await waitForOperation(operation.task_id)
        : operation;
      if (completed.error) throw new Error(completed.error);
      const result = completed.result as TaxonomyImportResult | null;
      if (!result) throw new Error("SQL import completed without a replacement result");
      setValidation(null);
      setStagingSchemas([]);
      setConfirming(false);
      onApplied?.();
      emitTaxonomyMutation({ kind: "replacement" });
      setMessage(formatTaxonomyImportApplyMessage(result.warnings));
    } catch (nextError) {
      setMessage("");
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  async function removeInput(input: PersistentSqlInput) {
    if (
      validation
      && !window.confirm("Removing this source will invalidate the current validation result.")
    ) {
      return;
    }
    setBusy(`Removing ${input.alias}`);
    setMessage("");
    setError("");
    try {
      const result = await removeSqlImportInput(input.alias);
      setInputs(result.inputs);
      setStagingSchemas([]);
      setValidation(null);
      setMessage(result.warnings.length > 0 ? result.warnings.join(" ") : "Data source removed.");
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  const primary = (
    <div className="sql-workbench-primary">
      <ResizablePanels
        className="sql-import-workbench"
        initialSize={250}
        minFirst={180}
        minSecond={320}
        separatorLabel="Resize Input sources"
        stateKey="sql-import.inputs"
        first={<SqlInputList inputs={inputs} workspaceSchemas={stagingSchemas} databaseSchemas={databaseSchemas} busy={Boolean(busy) || loadingWorkbench} operation={busy} onAdd={addInput} onRemove={removeInput} />}
        second={(<div className="sql-import-editor">
          <CodeEditor language="sql" ariaLabel="SQL import SQL" value={sql} onChange={(value) => {
            setSql(value);
            setValidation(null);
          }} />
        </div>)}
      />
      {error ? (
        <div className="inline-error sql-import-status" role="alert">{error}</div>
      ) : operation && sqlOperationProgress(operation) ? (
        <div className="sql-import-progress" role="status" aria-live="polite">
          <LoaderCircle className="spin" size={15} />
          <strong>{sqlOperationProgress(operation)}</strong>
        </div>
      ) : loadingWorkbench ? (
        <div className="sql-import-progress" role="status" aria-live="polite">
          <LoaderCircle className="spin" size={15} />
          <strong>Loading taxonomy database workspace...</strong>
        </div>
      ) : message ? (
        <div className="editor-message sql-import-status">{message}</div>
      ) : null}
    </div>
  );

  const output = validation ? (
    <div className="sql-import-validation">
      <div className="sql-import-metadata-grid">
        <Metric label="Candidate taxa" value={String(validation.taxa_count)} />
        <Metric label="Normalization changes" value={String(validation.normalization_changes)} />
        <Metric label="Warnings" value={String(validation.total_warning_count)} />
        <Metric label="Errors" value={String(validation.total_error_count)} />
      </div>
      <div className="validation-counts">
        {validation.name_counts.map((item) => <span key={item.name_type}>{item.name_type}: {item.count}</span>)}
      </div>
      <VirtualList
        className="validation-issues"
        items={[...validation.errors, ...validation.warnings]}
        rowHeight={SQL_IMPORT_VALIDATION_ISSUE_ROW_HEIGHT}
        itemKey={(item, index) => `${item.code}:${item.row_identifier}:${index}`}
        renderItem={(item) => {
          const row = sqlImportValidationIssueRow(item);
          return (
            <div className="validation-issue">
              <span className="validation-issue-message" title={row.message}>{row.message}</span>
              <code className="validation-issue-context" title={row.context}>{row.context}</code>
            </div>
          );
        }}
      />
    </div>
  ) : null;

  return (
    <div aria-hidden={!active} className={`sql-import-settings${active ? "" : " inactive"}`}>
      <SectionHeader
        title="SQL Import"
        detail="Build, validate, and apply a replacement taxonomy database."
        actions={(
          <>
            <Button onClick={() => setHelpOpen(true)}><CircleQuestionMark size={13} />Help</Button>
            <Button disabled={Boolean(busy) || loadingWorkbench || !sql.trim()} onClick={() => void validate()}>
              <ShieldCheck size={13} />{busy === "Validating SQL import" ? "Validating..." : "Validate"}
            </Button>
            <Button variant="primary" disabled={Boolean(busy) || loadingWorkbench || !validation?.can_apply} onClick={() => setConfirming(true)}>
              <Send size={13} />Apply
            </Button>
          </>
        )}
      />
      {output ? (
        <ResizablePanels
          className="sql-output-split"
          direction="vertical"
          initialRatio={0.55}
          minFirst={250}
          minSecond={150}
          separatorLabel="Resize validation output"
          stateKey="sql-import.output"
          first={primary}
          second={output}
        />
      ) : primary}
      {helpOpen && <SqlEnumHelpModal onClose={() => setHelpOpen(false)} />}
      {confirming && (
        <Modal
          title="Apply SQL import"
          dismissible={!busy}
          onClose={() => setConfirming(false)}
          actions={(
            <>
              <Button disabled={Boolean(busy)} onClick={() => setConfirming(false)}>Cancel</Button>
              <Button variant="primary" disabled={Boolean(busy)} onClick={() => void apply()}>
                {busy === "Applying SQL import" ? "Applying..." : "Replace taxonomy"}
              </Button>
            </>
          )}
        >
          <p>This replaces the taxonomy database, clears taxonomy history, and schedules every registered photo library for remapping.</p>
          <p>The validated candidate has {validation?.taxa_count ?? 0} taxa.</p>
        </Modal>
      )}
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div><span>{label}</span><strong title={value}>{value}</strong></div>;
}
