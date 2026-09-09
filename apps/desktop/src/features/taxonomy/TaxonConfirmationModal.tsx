import type { TaxonNameDetail } from "../../api/taxonomy";
import { Button, Modal } from "../../shared/ui";
import type { TaxonNameGroupKind } from "./taxonEditing";

export type TaxonConfirmation =
  | {
      kind: "promote-name";
      group: TaxonNameGroupKind;
      record: TaxonNameDetail;
    }
  | {
      kind: "delete-name";
      group: TaxonNameGroupKind;
      record: TaxonNameDetail;
    }
  | { kind: "delete-taxon" };

export function TaxonConfirmationModal({
  confirmation,
  taxonLabel,
  busy,
  error,
  onClose,
  onConfirm,
}: {
  confirmation: TaxonConfirmation;
  taxonLabel: string;
  busy: boolean;
  error: string;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const content = confirmationContent(confirmation, taxonLabel);
  return (
    <Modal
      title={content.title}
      dismissible={!busy}
      onClose={onClose}
      actions={(
        <>
          <Button disabled={busy} onClick={onClose}>Cancel</Button>
          <Button
            className={content.danger ? "taxonomy-danger-button" : ""}
            variant={content.danger ? "secondary" : "primary"}
            disabled={busy}
            onClick={onConfirm}
          >
            {busy ? content.busyLabel : content.confirmLabel}
          </Button>
        </>
      )}
    >
      {content.paragraphs.map((paragraph) => <p key={paragraph}>{paragraph}</p>)}
      {error ? <div className="inline-error" role="alert">{error}</div> : null}
    </Modal>
  );
}

function confirmationContent(confirmation: TaxonConfirmation, taxonLabel: string) {
  if (confirmation.kind === "delete-taxon") {
    return {
      title: "Delete taxon",
      confirmLabel: "Delete taxon",
      busyLabel: "Deleting...",
      danger: true,
      paragraphs: [
        `Delete ${taxonLabel} and all of its name records?`,
        "Only a taxon without direct children can be deleted. Affected photo mappings will be recalculated.",
      ],
    };
  }
  if (confirmation.kind === "promote-name") {
    const acceptedKind = confirmation.group === "synonym"
      ? "science name"
      : confirmation.group === "zh_alias"
        ? "Chinese name"
        : "English name";
    return {
      title: "Promote taxonomy name",
      confirmLabel: "Promote",
      busyLabel: "Promoting...",
      danger: false,
      paragraphs: [
        `Promote "${confirmation.record.name}" to the ${acceptedKind}?`,
        "The current main name becomes an alias or synonym. Record IDs, authority, and source stay with their records.",
      ],
    };
  }
  return {
    title: "Delete taxonomy name",
    confirmLabel: "Delete name",
    busyLabel: "Deleting...",
    danger: true,
    paragraphs: [
      `Delete "${confirmation.record.name}" from ${taxonLabel}?`,
      "This removes only the selected name record.",
    ],
  };
}
