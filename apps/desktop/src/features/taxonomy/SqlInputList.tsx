import { ChevronDown, ChevronRight, Database, DatabasePlus, FilePlusCorner, LoaderCircle, Table2, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { selectCsvFile, selectSqliteDatabase } from "../../api/dialogs";
import type { PersistentSqlInput, SqlSourceSchema } from "../../api/customSql";
import { Button, IconButton, Modal } from "../../shared/ui";
import { sqlInputAliasError, suggestedSqlInputAlias } from "./sqlInputAlias";
import { internalDatabaseSchemas, toggleSqlSourceGroup, type SqlSourceGroup } from "./sqlSourceSidebar";

type PendingSqlInput = {
  kind: "csv" | "sqlite";
  path: string;
  alias: string;
};

export function SqlInputList({
  inputs,
  workspaceSchemas,
  databaseSchemas,
  busy,
  operation = "",
  onAdd,
  onRemove,
}: {
  inputs: PersistentSqlInput[];
  workspaceSchemas: SqlSourceSchema[];
  databaseSchemas: SqlSourceSchema[];
  busy: boolean;
  operation?: string;
  onAdd: (kind: "csv" | "sqlite", alias: string, path: string) => Promise<void>;
  onRemove: (input: PersistentSqlInput) => Promise<void>;
}) {
  const [pending, setPending] = useState<PendingSqlInput | null>(null);
  const [adding, setAdding] = useState(false);
  const [expandedGroup, setExpandedGroup] = useState<SqlSourceGroup | null>("inputs");
  const aliasInputRef = useRef<HTMLInputElement>(null);
  const aliasError = pending ? sqlInputAliasError(pending.alias, inputs) : "";
  const internalSchemas = useMemo(
    () => internalDatabaseSchemas(databaseSchemas),
    [databaseSchemas],
  );

  useEffect(() => {
    if (!pending) return;
    aliasInputRef.current?.focus();
    aliasInputRef.current?.select();
  }, [pending?.path]);

  async function choose(kind: "csv" | "sqlite") {
    const path = kind === "csv" ? await selectCsvFile() : await selectSqliteDatabase();
    if (!path) return;
    setPending({ kind, path, alias: suggestedSqlInputAlias(path, inputs) });
  }

  async function confirmAdd() {
    if (!pending || aliasError || adding || busy) return;
    setAdding(true);
    try {
      await onAdd(pending.kind, pending.alias.trim(), pending.path);
      setPending(null);
    } finally {
      setAdding(false);
    }
  }

  return (
    <aside className={`sql-sources sql-source-sidebar ${expandedGroup ? `${expandedGroup}-expanded` : "groups-collapsed"}`}>
      <div className={`sql-source-group${expandedGroup === "inputs" ? " expanded" : ""}`}>
        <header className="sql-source-group-header">
          <button
            type="button"
            aria-expanded={expandedGroup === "inputs"}
            onClick={() => setExpandedGroup((current) => toggleSqlSourceGroup(current, "inputs"))}
          >
            {expandedGroup === "inputs" ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <strong>Input sources</strong>
          </button>
          <span className="sql-source-group-actions">
            <IconButton aria-label="Add CSV" size="small" disabled={busy} title="Add CSV" onClick={() => void choose("csv")}>
              <FilePlusCorner size={13} />
            </IconButton>
            <IconButton aria-label="Add SQLite" size="small" disabled={busy} title="Add SQLite" onClick={() => void choose("sqlite")}>
              <DatabasePlus size={13} />
            </IconButton>
          </span>
        </header>
        {expandedGroup === "inputs" && (
          <div className="sql-source-group-body">
            {workspaceSchemas.length === 0 && inputs.length === 0 && <span className="sql-source-empty">No data sources</span>}
            {workspaceSchemas.map((schema) => (
              <section className="sql-source-card sql-schema-card" key={schema.alias}>
                <header>
                  <span><Database size={12} /><b>{schema.alias}</b></span>
                  <i>STAGING</i>
                </header>
                <SqlSourceSchemaObjects schema={schema} />
              </section>
            ))}
            {inputs.map((input) => {
              const removing = operation === `Removing ${input.alias}`;
              return (
                <section className="sql-source-card" key={input.alias} aria-busy={removing}>
                  <header>
                    <span><b>{input.alias}</b><i>{input.kind.toUpperCase()}</i></span>
                    <IconButton
                      aria-label={removing ? `Removing ${input.alias}` : `Remove ${input.alias}`}
                      size="small"
                      title={removing ? "Removing..." : `Remove ${input.alias}`}
                      disabled={busy}
                      onClick={() => void onRemove(input)}
                    >
                      {removing ? <LoaderCircle className="spin" size={12} /> : <Trash2 size={12} />}
                    </IconButton>
                  </header>
                  <code title={input.original_path}>{input.original_path}</code>
                  <small className={input.available ? "available" : "unavailable"}>
                    {input.available ? "Stored copy available" : "Stored copy unavailable"}
                  </small>
                  <SqlSourceSchemaObjects schema={input.schema} />
                </section>
              );
            })}
          </div>
        )}
      </div>
      <div className={`sql-source-group${expandedGroup === "tables" ? " expanded" : ""}`}>
        <header className="sql-source-group-header">
          <button
            type="button"
            aria-expanded={expandedGroup === "tables"}
            onClick={() => setExpandedGroup((current) => toggleSqlSourceGroup(current, "tables"))}
          >
            {expandedGroup === "tables" ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <strong>All accessible tables</strong>
          </button>
        </header>
        {expandedGroup === "tables" && (
          <div className="sql-source-group-body">
            {internalSchemas.length === 0 && <span className="sql-source-empty">No accessible tables</span>}
            {internalSchemas.map((schema) => (
              <section className="sql-source-card sql-schema-card" key={schema.alias}>
                <header>
                  <span><Database size={12} /><b>{schema.alias}</b></span>
                  <i>{schema.objects.length} objects</i>
                </header>
                <SqlSourceSchemaObjects schema={schema} />
              </section>
            ))}
          </div>
        )}
      </div>
      {pending && (
        <Modal
          title={`Add ${pending.kind === "sqlite" ? "SQLite" : "CSV"} input`}
          dismissible={!adding}
          onClose={() => setPending(null)}
          actions={(
            <>
              <Button disabled={adding} onClick={() => setPending(null)}>Cancel</Button>
              <Button variant="primary" disabled={adding || Boolean(aliasError)} onClick={() => void confirmAdd()}>
                {adding ? "Adding..." : "Add input"}
              </Button>
            </>
          )}
        >
          <label className="sql-input-alias-field">
            <span>SQL access name</span>
            <input
              ref={aliasInputRef}
              value={pending.alias}
              disabled={adding}
              aria-invalid={Boolean(aliasError)}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              onChange={(event) => setPending({ ...pending, alias: event.target.value })}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void confirmAdd();
                }
              }}
            />
          </label>
          <div className="sql-input-selected-path"><span>Selected file</span><code title={pending.path}>{pending.path}</code></div>
          {aliasError && <div className="inline-error" role="alert">{aliasError}</div>}
        </Modal>
      )}
    </aside>
  );
}

export function SqlSourceSchemaObjects({ schema }: { schema: SqlSourceSchema }) {
  return schema.objects.map((object) => (
    <details key={`${object.object_type}:${object.name}`}>
      <summary><Table2 size={12} />{object.name}</summary>
      {object.columns.map((column) => (
        <span key={column.name}>{column.name}<i>{column.declared_type ?? "untyped"}</i></span>
      ))}
    </details>
  ));
}
