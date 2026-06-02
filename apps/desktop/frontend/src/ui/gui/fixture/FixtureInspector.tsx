import type { FixtureDocumentDto } from "../../../bindings";
import { commands } from "../../../api";
import { runSnapshotCommand } from "../../../store";
import { InspectorScrollArea } from "../InspectorScrollArea";

export function FixtureInspector({ document, selected }: { document: FixtureDocumentDto; selected: string | null }) {
  const fixture = document.fixtures.find((candidate) => candidate.objectKey === document.selectedObjectKey) ?? document.fixtures[0];
  return (
    <InspectorScrollArea>
      <h2>Fixture</h2>
      {fixture !== undefined ? (
        <>
          <label>Name<input readOnly value={fixture.name} /></label>
          <label>
            Bulb
            <input
              type="number"
              min={0.001}
              step="any"
              defaultValue={fixture.bulbDiameterMeters}
              onBlur={(event) =>
                void runSnapshotCommand(() =>
                  commands.applyFixtureGuiEdit({
                    type: "updateBulbDiameter",
                    objectKey: fixture.objectKey,
                    bulbDiameterMeters: Number(event.currentTarget.value)
                  })
                )
              }
            />
          </label>
          <label>Geometry<input readOnly value={fixture.geometrySummary} /></label>
          <p>{selected !== null && selected.startsWith("point:") ? `Point ${Number(selected.split(":")[1]) + 1}` : "Select a point."}</p>
        </>
      ) : (
        <p>No fixture.</p>
      )}
    </InspectorScrollArea>
  );
}
