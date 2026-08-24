import type { SequenceEditorDocument } from "../../../types";

export function RecoverySequenceView({ document }: { document: SequenceEditorDocument }) {
  const timelineItems = document.recoveryItems.filter((item) => item.placement.type === "timeline");
  const graphItems = document.recoveryItems.filter((item) => item.placement.type === "graph");
  const tracks = recoveryTracks(document);

  return (
    <section className="recovery-sequence-view" aria-label="Read-only sequence recovery view">
      <header>
        <div>
          <strong>{document.objectKey}</strong>
          <span>{formatSeconds(document.durationSeconds)} · {document.frameRate} fps</span>
        </div>
        <span>Only items with complete source placement are shown.</span>
      </header>
      <div className="recovery-timeline">
        {tracks.map((track, trackIndex) => (
          <div className="recovery-track" key={track.key}>
            <span className="recovery-track-label">{track.label}</span>
            <div className="recovery-track-content">
              {timelineItems
                .filter((item) => timelineTrackKey(item) === track.key)
                .map((item) => {
                  if (item.placement.type !== "timeline") return null;
                  const left = (item.placement.startSeconds / document.durationSeconds) * 100;
                  const width = (item.placement.durationSeconds / document.durationSeconds) * 100;
                  return (
                    <div
                      className={`recovery-placeholder recovery-${item.kind}`}
                      key={`${item.kind}:${item.id}:${trackIndex}`}
                      style={{
                        left: `${left}%`,
                        width: `${width}%`
                      }}
                      title={item.message ?? undefined}
                    >
                      {item.kind} {item.id}
                    </div>
                  );
                })}
            </div>
          </div>
        ))}
        {tracks.length === 0 && (
          <p className="recovery-empty-placement">No timeline item has complete placement.</p>
        )}
      </div>
      {graphItems.length > 0 && (
        <section className="recovery-graph-items">
          <h3>Placed graph items</h3>
          <div>
            {graphItems.map((item) => {
              if (item.placement.type !== "graph") return null;
              return (
                <article key={`${item.kind}:${item.id}`}>
                  <strong>{item.kind} {item.id}</strong>
                  <span>x {item.placement.x}, y {item.placement.y}</span>
                </article>
              );
            })}
          </div>
        </section>
      )}
    </section>
  );
}

function recoveryTracks(document: SequenceEditorDocument) {
  const values = new Map<string, string>();
  for (const layer of document.layers) {
    values.set(`layer:${layer.id}`, layer.name);
  }
  for (const item of document.recoveryItems) {
    if (item.placement.type !== "timeline" || item.placement.lane.type !== "lane") continue;
    values.set(`lane:${item.placement.lane.laneIndex}`, `Lane ${item.placement.lane.laneIndex}`);
  }
  return [...values.entries()].map(([key, label]) => ({ key, label }));
}

function timelineTrackKey(item: SequenceEditorDocument["recoveryItems"][number]): string | null {
  if (item.placement.type !== "timeline") return null;
  return item.placement.lane.type === "layer"
    ? `layer:${item.placement.lane.layerId}`
    : `lane:${item.placement.lane.laneIndex}`;
}

function formatSeconds(seconds: number): string {
  return `${seconds.toFixed(3)}s`;
}
