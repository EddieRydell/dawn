import type { FixtureDocumentDto } from "../../../bindings";
import { commands } from "../../../api";
import { runRuntimeCommand } from "../../../store";
import { InspectorScrollArea } from "../InspectorScrollArea";
import type { GuiFocus } from "../shared";

export function FixtureInspector({ document, selected }: { document: FixtureDocumentDto; selected: GuiFocus }) {
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
                void runRuntimeCommand(() =>
                  commands.applyFixtureDocumentEdit({
                    type: "updateBulbDiameter",
                    objectKey: fixture.objectKey,
                    bulbDiameterMeters: Number(event.currentTarget.value)
                  })
                )
              }
            />
          </label>
          <label>Geometry<input readOnly value={fixture.geometrySummary} /></label>
          <p>{selected?.type === "point" ? `Point ${selected.index + 1}` : "Select a point."}</p>
        </>
      ) : (
        <p>No fixture.</p>
      )}
    </InspectorScrollArea>
  );
}
