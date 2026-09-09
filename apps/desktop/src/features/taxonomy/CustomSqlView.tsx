import { CircleQuestionMark, Download, Play } from "lucide-react";
import { Fragment, useEffect, useMemo, useState } from "react";
import {
  addCustomSqlInput,
  executeCustomSql,
  exportCustomSqlQuery,
  getCustomTaxonomySql,
  listCustomSqlDatabaseSchemas,
  listCustomSqlInputs,
  removeCustomSqlInput,
  type CustomSqlExecutionResult,
  type PersistentSqlInput,
  type SqlExportResult,
  type SqlResultSet,
  type SqlSourceSchema,
  type SqlValue,
} from "../../api/customSql";
import { errorMessage } from "../../api/common";
import { waitForOperation, type OperationState } from "../../api/tasks";
import { operationResult } from "../../app/backgroundTaskResult";
import { selectCsvDestination } from "../../api/dialogs";
import { CodeEditor } from "../../shared/CodeEditor";
import { ResizablePanels } from "../../shared/ResizablePanels";
import { Busy, Button, SectionHeader, VirtualList } from "../../shared/ui";
import { SqlInputList } from "./SqlInputList";
import { SqlEnumHelpModal } from "./TaxonomyHelpModal";
import {
  formatAffectedRows,
  formatRowCount,
  formatSqlExecutionStatus,
  maxSqlResultColumnCount,
  sqlResultSetHeight,
  sqlResultTableMinWidth,
  sqlStatementOutputs,
  type CustomSqlExecutionSnapshot,
} from "./sqlResults";
import { resolveSqlWorkbenchLoads } from "./sqlWorkbenchLoading";
import { emitTaxonomyMutation } from "./taxonomyMutations";
import { sqlOperationProgress } from "./sqlOperationProgress";

export function CustomSqlView({
  onStatus,
  taskOwnerId,
  mutationDisabled = false,
}: {
  onStatus: (message: string) => void;
  taskOwnerId: string;
  mutationDisabled?: boolean;
}) {
  const [sql, setSql] = useState("");
  const [inputs, setInputs] = useState<PersistentSqlInput[]>([]);
  const [databaseSchemas, setDatabaseSchemas] = useState<SqlSourceSchema[]>([]);
  const [execution, setExecution] = useState<CustomSqlExecutionSnapshot | null>(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [loadingWorkbench, setLoadingWorkbench] = useState(true);
  const [helpOpen, setHelpOpen] = useState(false);
  const [operation, setOperation] = useState<OperationState | null>(null);

  useEffect(() => {
    void Promise.allSettled([
      getCustomTaxonomySql(),
      listCustomSqlInputs(),
      listCustomSqlDatabaseSchemas(),
    ])
      .then(([sqlResult, inputsResult, schemasResult]) => {
        const loaded = resolveSqlWorkbenchLoads(sqlResult, inputsResult);
        if (loaded.sql !== undefined) setSql(loaded.sql);
        if (loaded.inputs !== undefined) setInputs(loaded.inputs);
        if (schemasResult.status === "fulfilled") setDatabaseSchemas(schemasResult.value);
        const schemasError = schemasResult.status === "rejected" ? errorMessage(schemasResult.reason) : "";
        setError([loaded.error, schemasError].filter(Boolean).join(" "));
      })
      .finally(() => setLoadingWorkbench(false));
  }, []);

  async function addInput(kind: "csv" | "sqlite", alias: string, path: string) {
    setBusy("Adding data source");
    setError("");
    try {
      const result = await addCustomSqlInput(kind, alias, path);
      setInputs(result.inputs);
      if (result.warnings.length > 0) onStatus(result.warnings.join(" "));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  async function removeInput(input: PersistentSqlInput) {
    setBusy(`Removing ${input.alias}`);
    setError("");
    try {
      const result = await removeCustomSqlInput(input.alias);
      setInputs(result.inputs);
      if (result.warnings.length > 0) onStatus(result.warnings.join(" "));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  async function execute() {
    const executedSql = sql;
    setBusy("Executing SQL");
    setError("");
    setOperation(null);
    try {
      const started = await executeCustomSql(executedSql, taskOwnerId);
      setOperation(started);
      const completed = started.task_id && ["queued", "running"].includes(started.state)
        ? await waitForOperation(started.task_id, setOperation)
        : started;
      const next = operationResult<CustomSqlExecutionResult>(completed, started.task_id);
      setExecution({ sql: executedSql, result: next });
      if (next.operation_id !== null) emitTaxonomyMutation();
      onStatus(formatSqlExecutionStatus(next));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
      setOperation(null);
    }
  }

  async function exportQuery(statementIndex: number) {
    if (!execution) return;
    const destination = await selectCsvDestination(`taxonomy-query-statement-${statementIndex}.csv`);
    if (!destination) return;
    const operation = `Exporting statement ${statementIndex}`;
    setBusy(operation);
    setError("");
    try {
      const started = await exportCustomSqlQuery(
        execution.sql,
        statementIndex,
        destination,
        taskOwnerId,
      );
      const completed = started.task_id && ["queued", "running"].includes(started.state)
        ? await waitForOperation(started.task_id)
        : started;
      const exported = operationResult<SqlExportResult>(completed, started.task_id);
      onStatus(`Exported ${exported.row_count} rows to ${exported.path}`);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy("");
    }
  }

  const workbench = (
    <ResizablePanels
      className="custom-sql-workbench"
      initialSize={250}
      minFirst={180}
      minSecond={320}
      separatorLabel="Resize Input sources"
      stateKey="custom-sql.inputs"
      first={<SqlInputList inputs={inputs} workspaceSchemas={[]} databaseSchemas={databaseSchemas} busy={Boolean(busy) || loadingWorkbench} operation={busy} onAdd={addInput} onRemove={removeInput} />}
      second={(<div className="custom-sql-editor">
        <CodeEditor language="sql" ariaLabel="Custom taxonomy SQL" value={sql} onChange={setSql} />
      </div>)}
    />
  );

  const primary = (
    <div className="sql-workbench-primary">
      {workbench}
      {error ? (
        <div className="inline-error" role="alert">{error}</div>
      ) : operation && sqlOperationProgress(operation) ? (
        <div className="editor-message" role="status" aria-live="polite">
          <Busy label={sqlOperationProgress(operation)} />
        </div>
      ) : loadingWorkbench ? (
        <div className="editor-message" role="status" aria-live="polite">
          <Busy label="Loading custom SQL workspace..." />
        </div>
      ) : null}
    </div>
  );

  const result = execution?.result ?? null;
  const executionColumnCount = result ? maxSqlResultColumnCount(result.result_sets) : 1;
  const statementOutputs = result ? sqlStatementOutputs(result) : [];
  const output = result ? (
    <div className="sql-results">
      {statementOutputs.map((statement) => (
        <Fragment key={statement.statementIndex}>
          {statement.resultSet ? (
            <SqlResultTable
              result={statement.resultSet}
              affectedRows={statement.affectedRows}
              executionColumnCount={executionColumnCount}
              exportAllowed={statement.exportAllowed}
              busy={busy}
              onExport={exportQuery}
            />
          ) : statement.affectedRows !== null ? (
            <p className="sql-message">
              Statement {statement.statementIndex} &middot; {formatAffectedRows(statement.affectedRows)}
            </p>
          ) : null}
        </Fragment>
      ))}
    </div>
  ) : null;

  return (
    <div className="custom-sql-view">
      <SectionHeader
        title="Custom SQL"
        detail="Execute typed SQL against taxonomy and file-path data sources"
        actions={(
          <>
            <Button onClick={() => setHelpOpen(true)}><CircleQuestionMark size={13} />Help</Button>
            <Button variant="primary" disabled={Boolean(busy) || loadingWorkbench || !sql.trim() || mutationDisabled} onClick={() => void execute()}>
              <Play size={13} />{busy === "Executing SQL" ? "Running..." : "Run"}
            </Button>
          </>
        )}
      />
      {output ? (
        <ResizablePanels
          className="sql-output-split"
          direction="vertical"
          initialRatio={0.55}
          minFirst={230}
          minSecond={130}
          separatorLabel="Resize SQL output"
          stateKey="custom-sql.output"
          first={primary}
          second={output}
        />
      ) : primary}
      {helpOpen && <SqlEnumHelpModal onClose={() => setHelpOpen(false)} />}
    </div>
  );
}

function SqlResultTable({
  result,
  affectedRows,
  executionColumnCount,
  exportAllowed,
  busy,
  onExport,
}: {
  result: SqlResultSet;
  affectedRows: number | null;
  executionColumnCount: number;
  exportAllowed: boolean;
  busy: string;
  onExport: (statementIndex: number) => Promise<void>;
}) {
  const template = useMemo(
    () => `repeat(${Math.max(result.columns.length, 1)}, minmax(130px, 1fr))`,
    [result.columns.length],
  );
  const exportOperation = `Exporting statement ${result.statement_index}`;
  const resultHeight = sqlResultSetHeight(result.rows.length);
  return (
    <section className="sql-result-set" style={{ height: `min(${resultHeight}px, 320px, 42vh)` }}>
      <header className="sql-result-set-header">
        <strong>Statement {result.statement_index}</strong>
        <span>
          {formatRowCount(result.rows.length)}
          {result.truncated ? <> &middot; Preview truncated</> : null}
          {affectedRows !== null ? <> &middot; {formatAffectedRows(affectedRows)}</> : null}
        </span>
        {exportAllowed ? (
          <Button disabled={Boolean(busy)} onClick={() => void onExport(result.statement_index)}>
            <Download size={13} />{busy === exportOperation ? "Exporting..." : "Export CSV"}
          </Button>
        ) : null}
      </header>
      <div className="sql-result-scroll">
        <div
          className="sql-result-table"
          style={{ minWidth: sqlResultTableMinWidth(executionColumnCount) }}
        >
          <div className="sql-result-header" style={{ gridTemplateColumns: template }}>
            {result.columns.map((column) => (
              <span key={column.name}>{column.name}<i>{column.declared_type ?? ""}</i></span>
            ))}
          </div>
          <VirtualList
            className="sql-result-rows"
            items={result.rows}
            rowHeight={32}
            itemKey={(_, index) => index}
            renderItem={(row) => (
              <div className="sql-result-row" style={{ gridTemplateColumns: template }}>
                {row.map((value, index) => <code key={index}>{displaySqlValue(value)}</code>)}
              </div>
            )}
          />
        </div>
      </div>
    </section>
  );
}

function displaySqlValue(value: SqlValue): string {
  if (value.type === "null") return "NULL";
  if (value.type === "blob") return `[blob/base64] ${value.value}`;
  return String(value.value);
}
