import {
  CircleArrowUp,
  Plus,
  Save,
  SquarePen,
  Trash2,
  X,
} from "lucide-react";
import { useState, type FormEvent } from "react";
import type {
  SaveTaxonNameGroupInput,
  TaxonNameDetail,
} from "../../api/taxonomy";
import { Button, IconButton } from "../../shared/ui";
import {
  buildTaxonNameGroupSaveInput,
  canPromoteTaxonName,
  createBlankTaxonNameDraftRow,
  createTaxonNameDraftRows,
  isPrimaryTaxonNameGroup,
  taxonNameGroupLabels,
  type TaxonNameDraftRow,
  type TaxonNameGroupKind,
} from "./taxonEditing";

export function TaxonNameGroupEditor({
  taxonId,
  kind,
  records,
  primaryExists,
  deleteAllowed,
  active,
  busy,
  error,
  disabled,
  onStartEditing,
  onCancelEditing,
  onSave,
  onPromote,
  onDelete,
}: {
  taxonId: number;
  kind: TaxonNameGroupKind;
  records: TaxonNameDetail[];
  primaryExists: boolean;
  deleteAllowed: boolean;
  active: boolean;
  busy: boolean;
  error: string;
  disabled: boolean;
  onStartEditing: () => void;
  onCancelEditing: () => void;
  onSave: (input: SaveTaxonNameGroupInput) => void;
  onPromote: (record: TaxonNameDetail) => void;
  onDelete: (record: TaxonNameDetail) => void;
}) {
  const [rows, setRows] = useState<TaxonNameDraftRow[]>([]);
  const [validationError, setValidationError] = useState("");
  const primary = isPrimaryTaxonNameGroup(kind);
  const promoteAllowed = canPromoteTaxonName(kind);
  const canAdd = primary ? records.length === 0 : primaryExists;
  const action = records.length > 0 ? "edit" : canAdd ? "add" : null;
  const formId = `taxonomy-name-group-${taxonId}-${kind}`;

  function startEditing() {
    const existingRows = createTaxonNameDraftRows(records);
    setRows(existingRows.length > 0 ? existingRows : [createBlankTaxonNameDraftRow()]);
    setValidationError("");
    onStartEditing();
  }

  function cancelEditing() {
    setValidationError("");
    onCancelEditing();
  }

  function updateRow(index: number, field: keyof TaxonNameDraftRow, value: string) {
    setRows((current) => current.map((row, rowIndex) => (
      rowIndex === index ? { ...row, [field]: value } : row
    )));
  }

  function addRow() {
    setRows((current) => [...current, createBlankTaxonNameDraftRow()]);
    setValidationError("");
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    setValidationError("");
    try {
      onSave(buildTaxonNameGroupSaveInput(taxonId, kind, rows));
    } catch (nextError) {
      setValidationError(nextError instanceof Error ? nextError.message : String(nextError));
    }
  }

  return (
    <section className={`taxonomy-name-group${active ? " editing" : ""}`} aria-busy={busy}>
      <header className="taxonomy-name-group-header">
        <h3>{taxonNameGroupLabels[kind]}</h3>
        <div className="taxonomy-name-group-actions">
          {active ? (
            <>
              <Button size="small" disabled={busy} onClick={cancelEditing}>
                <X size={13} /> Cancel
              </Button>
              <Button
                className="taxonomy-save-button"
                form={formId}
                size="small"
                type="submit"
                disabled={busy}
              >
                <Save size={13} /> {busy ? "Saving..." : "Save"}
              </Button>
            </>
          ) : action ? (
            <Button size="small" disabled={disabled} onClick={startEditing}>
              {action === "edit" ? <SquarePen size={13} /> : <Plus size={13} />}
              {action === "edit" ? "Edit" : "Add"}
            </Button>
          ) : null}
        </div>
      </header>

      {active ? (
        <form className="taxonomy-name-group-form" id={formId} onSubmit={submit}>
          {rows.map((row, index) => (
            <article
              className={`taxonomy-name-record taxonomy-name-record-edit${row.nameId === null ? " new" : ""}`}
              key={row.nameId ?? `new-${index}`}
            >
              {row.nameId === null ? (
                <label className="taxonomy-name-edit-field taxonomy-name-field">
                  <span>Name</span>
                  <input
                    autoFocus={index === 0 && records.length === 0}
                    disabled={busy}
                    value={row.name}
                    onChange={(event) => updateRow(index, "name", event.target.value)}
                  />
                </label>
              ) : <strong>{row.name}</strong>}
              <div className="taxonomy-name-metadata-edit">
                <label className="taxonomy-name-edit-field">
                  <span>Authority</span>
                  <input
                    disabled={busy}
                    value={row.authorityYear}
                    onChange={(event) => updateRow(index, "authorityYear", event.target.value)}
                  />
                </label>
                <label className="taxonomy-name-edit-field">
                  <span>Source</span>
                  <input
                    disabled={busy}
                    value={row.source}
                    onChange={(event) => updateRow(index, "source", event.target.value)}
                  />
                </label>
              </div>
            </article>
          ))}
          {!primary && canAdd ? (
            <Button className="taxonomy-name-add-row" size="small" disabled={busy} onClick={addRow}>
              <Plus size={13} /> Add
            </Button>
          ) : null}
          {validationError || error ? (
            <div className="inline-error" role="alert">{validationError || error}</div>
          ) : null}
        </form>
      ) : records.length === 0 ? (
        <span className="taxonomy-name-empty">-</span>
      ) : records.map((record) => (
        <article className="taxonomy-name-record" key={record.name_id}>
          <header>
            <strong>{record.name}</strong>
            {promoteAllowed || deleteAllowed ? (
              <div className="taxonomy-name-actions">
                {promoteAllowed ? (
                  <IconButton
                    size="small"
                    aria-label={`Promote ${record.name}`}
                    title="Promote"
                    disabled={disabled}
                    onClick={() => onPromote(record)}
                  >
                    <CircleArrowUp size={13} />
                  </IconButton>
                ) : null}
                {deleteAllowed ? (
                  <IconButton
                    size="small"
                    className="taxonomy-danger-action"
                    aria-label={`Delete name ${record.name}`}
                    title="Delete name"
                    disabled={disabled}
                    onClick={() => onDelete(record)}
                  >
                    <Trash2 size={13} />
                  </IconButton>
                ) : null}
              </div>
            ) : null}
          </header>
          <dl>
            <div><dt>Authority</dt><dd>{record.authority_year ?? "-"}</dd></div>
            <div><dt>Source</dt><dd>{record.source ?? "-"}</dd></div>
          </dl>
        </article>
      ))}
    </section>
  );
}
