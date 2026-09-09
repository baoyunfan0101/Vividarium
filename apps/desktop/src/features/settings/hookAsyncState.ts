import type { NamingHookKind, NamingHookTestCase } from "../../api/settings";

export type NamingHookSnapshot = {
  script: string;
  cases: NamingHookTestCase[];
};

export type NamingHookSnapshots = Record<NamingHookKind, NamingHookSnapshot | null>;

export function canPresentHookResult(
  testedKind: NamingHookKind,
  activeKind: NamingHookKind,
  testedRevision: number,
  currentRevision: number,
): boolean {
  return testedKind === activeKind && testedRevision === currentRevision;
}

export function hookDraftMatchesSnapshot(
  script: string,
  cases: NamingHookTestCase[],
  snapshot: NamingHookSnapshot | null,
): boolean {
  return snapshot !== null
    && snapshot.script === script
    && JSON.stringify(snapshot.cases) === JSON.stringify(cases);
}

export function replaceTestedHookSnapshot(
  snapshots: NamingHookSnapshots,
  kind: NamingHookKind,
  snapshot: NamingHookSnapshot | null,
): NamingHookSnapshots {
  return { ...snapshots, [kind]: snapshot };
}
