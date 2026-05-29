import { useEffect, useRef } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { commands } from "../api";
import { installGlobalShortcuts } from "../commandRegistry";
import { runSnapshotCommand, subscribeToSnapshots, useAppStore } from "../store";
import type { AppSnapshotDto } from "../bindings";
import { EditorPane } from "./EditorPane";
import { ProjectTree } from "./ProjectTree";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";

export function App() {
  const { snapshot, error, hydrate } = useAppStore();

  useEffect(() => {
    void hydrate();
    const disposeShortcuts = installGlobalShortcuts();
    let disposeEvents: (() => void) | undefined;
    void subscribeToSnapshots().then((dispose) => {
      disposeEvents = dispose;
    });
    return () => {
      disposeShortcuts();
      disposeEvents?.();
    };
  }, [hydrate]);

  if (!snapshot) {
    return <div className="app-loading">Dawn</div>;
  }

  return (
    <div className="app-shell">
      <TitleBar />
      {error !== null && error !== "" && <div className="error-strip">{error}</div>}
      <main className="workbench">
        {snapshot.projectTreeVisible ? <ProjectTree snapshot={snapshot} /> : null}
        <EditorPane snapshot={snapshot} />
      </main>
      <SequenceAudioPlayback preview={snapshot.preview} />
      <StatusBar snapshot={snapshot} />
      {snapshot.projectRoot === null && (
        <div className="empty-project">
          <button onClick={() => void runSnapshotCommand(commands.openProjectDialog)}>Open Project</button>
        </div>
      )}
    </div>
  );
}

function SequenceAudioPlayback({ preview }: { preview: AppSnapshotDto["preview"] }) {
  const previewRef = useRef(preview);

  useEffect(() => {
    previewRef.current = preview;
  }, [preview]);

  useEffect(() => {
    const audio = new Audio();
    audio.preload = "auto";
    audio.style.display = "none";
    document.body.appendChild(audio);
    let activePath: string | null = null;
    let disposed = false;

    const sync = (state: AppSnapshotDto["preview"]) => {
      const audioPath = state.audio?.exists === true ? state.audio.resolvedPath : null;
      if (audioPath !== activePath) {
        activePath = audioPath;
        audio.pause();
        audio.removeAttribute("src");
        if (audioPath !== null) {
          audio.src = convertFileSrc(audioPath);
          audio.load();
        }
      }
      if (activePath === null) return;

      const targetSeconds = Math.max(0, state.positionMs / 1000);
      if (Number.isFinite(audio.duration) && targetSeconds > audio.duration + 0.05) {
        audio.pause();
        return;
      }
      const driftSeconds = Math.abs(audio.currentTime - targetSeconds);
      if (driftSeconds > 0.08) {
        audio.currentTime = targetSeconds;
      }
      if (state.isPlaying) {
        void audio.play().catch(() => undefined);
      } else {
        audio.pause();
      }
    };

    sync(previewRef.current);
    let dispose: (() => void) | undefined;
    void listen<AppSnapshotDto["preview"]>("preview_state_changed", (event) => {
      if (!disposed) sync(event.payload);
    }).then((listener) => {
      dispose = listener;
    });

    return () => {
      disposed = true;
      dispose?.();
      audio.pause();
      audio.remove();
    };
  }, []);

  return null;
}
