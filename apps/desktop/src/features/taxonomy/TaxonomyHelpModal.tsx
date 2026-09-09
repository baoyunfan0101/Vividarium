import { Modal } from "../../shared/ui";

export function SqlEnumHelpModal({ onClose }: { onClose: () => void }) {
  return (
    <Modal title="SQL taxonomy codes" width={440} onClose={onClose}>
      <div className="taxonomy-help">
        <CodeTable
          field="taxa.rank"
          values={[
            [1, "kingdom"],
            [2, "order"],
            [3, "family"],
            [4, "genus"],
            [5, "species"],
          ]}
        />
        <CodeTable
          field="taxon_names.name_type"
          values={[
            [1, "sci_name"],
            [2, "synonym"],
            [3, "zh_name"],
            [4, "zh_alias"],
            [5, "en_name"],
            [6, "en_alias"],
          ]}
        />
      </div>
    </Modal>
  );
}

export function FormattedUpdateHelpModal({ onClose }: { onClose: () => void }) {
  return (
    <Modal title="Formatted update rules" width={520} onClose={onClose}>
      <ol className="taxonomy-help-steps">
        <li><strong>Normalize.</strong> If species is filled and genus is blank, the first species word becomes genus.</li>
        <li><strong>Match the lowest rank.</strong> For the rank name and then each input synonym, match sci_name first. Only when that name has no sci_name candidate, try synonym.</li>
        <li><strong>Decide.</strong> One match updates immediately; zero creates; multiple matches use supplied ancestors from nearest to highest until unique.</li>
        <li><strong>Create the lineage.</strong> A new taxon requires its direct parent. Parent and ancestor resolution uses the same sci_name-first, synonym-fallback rule; missing parents are created recursively. Missing input or unresolved ambiguity fails the row.</li>
      </ol>
    </Modal>
  );
}

function CodeTable({
  field,
  values,
}: {
  field: string;
  values: Array<[number, string]>;
}) {
  return (
    <section className="taxonomy-help-code-group">
      <strong><code>{field}</code></strong>
      <div className="taxonomy-help-code-grid">
        {values.map(([code, meaning]) => (
          <div key={code}><code>{code}</code><span>{meaning}</span></div>
        ))}
      </div>
    </section>
  );
}
