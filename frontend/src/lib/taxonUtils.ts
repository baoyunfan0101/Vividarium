import { getMappingTaxon, type MappingNode, type Taxon } from "../api";

export async function lineageForNode(node: MappingNode): Promise<Taxon[]> {
  if (!node.taxon) {
    return [];
  }
  const lineage: Taxon[] = [node.taxon];
  let parentId = node.taxon.parent_id;
  while (parentId !== null) {
    const parentNode = await getMappingTaxon(parentId);
    if (!parentNode.taxon) {
      break;
    }
    lineage.unshift(parentNode.taxon);
    parentId = parentNode.taxon.parent_id;
  }
  return lineage;
}

export function taxonLabel(taxon: Taxon): string {
  return taxon.binomial_name ? `${taxon.name} / ${taxon.binomial_name}` : taxon.name;
}

export function taxonCrumbLabel(taxon: Taxon): string {
  return taxon.binomial_name ? `${taxon.name} (${taxon.binomial_name})` : taxon.name;
}
