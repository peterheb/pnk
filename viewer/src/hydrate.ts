// Style-pool hydration: the document carries deduped pools (styles.para[],
// styles.char[], per-table cellStyles[]) and nodes reference entries by
// index. Hydration resolves indexes -> objects once per document so the
// renderers keep working against plain style objects; absent index = default
// (empty style).

import type {
  CharStyle,
  DocumentEnvelope,
  ParaStyle,
  TableModel,
} from "../../model/src/shared";

/** A hydrated document: pools materialized for O(1) index lookups. */
export interface HydratedDoc extends DocumentEnvelope {
  paraStyles: ParaStyle[];
  charStyles: CharStyle[];
}

export function hydrate(doc: DocumentEnvelope): HydratedDoc {
  return {
    ...doc,
    paraStyles: doc.styles?.para ?? [],
    charStyles: doc.styles?.char ?? [],
  };
}

/** Document's para style for a node index; absent index = default. */
export function paraStyleOf(doc: HydratedDoc, index: number | undefined): ParaStyle | undefined {
  return index === undefined ? undefined : doc.paraStyles[index];
}

/** Document's char style for a node index; absent index = default. */
export function charStyleOf(doc: HydratedDoc, index: number | undefined): CharStyle | undefined {
  return index === undefined ? undefined : doc.charStyles[index];
}

/** Table's per-cell look for a cell index; absent index = table default. */
export function cellStyleOf(model: TableModel, index: number | undefined): TableModel["cellStyles"][number] | undefined {
  return index === undefined ? undefined : model.cellStyles?.[index];
}