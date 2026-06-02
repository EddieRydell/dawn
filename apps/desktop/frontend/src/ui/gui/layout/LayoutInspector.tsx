import type { LayoutDocumentDto } from "../../../bindings";
import { InspectorScrollArea } from "../InspectorScrollArea";
import { normalizeTransform } from "../shared";

export function LayoutInspector({ document, selected }: { document: LayoutDocumentDto; selected: string | null }) {
  const id = selected !== null && selected.startsWith("placement:") ? Number(selected.split(":")[1]) : null;
  const placement = document.fixtures.find((candidate) => candidate.id === id);
  const transform = placement === undefined ? null : normalizeTransform(placement.transform);
  return (
    <InspectorScrollArea>
      <h2>Layout</h2>
      {placement !== undefined && transform !== null ? (
        <>
          <label>Placement<input readOnly value={placement.name} /></label>
          <label>X<input readOnly value={transform.position.x} /></label>
          <label>Y<input readOnly value={transform.position.y} /></label>
          <label>Fixture<input readOnly value={placement.resolvedFixture.name} /></label>
        </>
      ) : (
        <p>Select a placement.</p>
      )}
    </InspectorScrollArea>
  );
}
