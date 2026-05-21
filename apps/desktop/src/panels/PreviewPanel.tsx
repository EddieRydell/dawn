import { Pause, Play } from "lucide-react";
import { useEffect } from "react";
import { useWorkbench } from "../store/workbenchStore";

export function PreviewPanel() {
  const playing = useWorkbench((state) => state.playing);
  const time = useWorkbench((state) => state.time);
  const frame = useWorkbench((state) => state.frame);
  const activeSequence = useWorkbench((state) => state.activeSequence);
  const setTime = useWorkbench((state) => state.setTime);
  const togglePlayback = useWorkbench((state) => state.togglePlayback);
  const renderFrame = useWorkbench((state) => state.renderFrame);

  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => setTime(useWorkbench.getState().time + 0.1), 100);
    return () => window.clearInterval(id);
  }, [playing, setTime]);

  useEffect(() => {
    void renderFrame();
  }, [activeSequence, time, renderFrame]);

  return (
    <section className="panel preview">
      <div className="transport">
        <button title={playing ? "Pause" : "Play"} onClick={togglePlayback}>
          {playing ? <Pause size={18} /> : <Play size={18} />}
        </button>
        <input type="range" min="0" max="30" step="0.05" value={time} onChange={(event) => setTime(Number(event.target.value))} />
        <span>{time.toFixed(2)}s</span>
      </div>
      <canvas className="preview-canvas" />
      <div className="frame-readout">{frame ? `${frame.pixels} pixels, ${frame.fixtureSpans} spans` : "No rendered frame"}</div>
    </section>
  );
}
