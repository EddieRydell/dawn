import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { commands } from "../api";
import { runRuntimeCommand, useAppStore } from "../store";

const EXPORT_FSEQ_EVENT = "dawn:export-fseq";
const DEFAULT_STEP_MS = 50;

export function ExportFseqDialog() {
  const [open, setOpen] = useState(false);
  const [stepMs, setStepMs] = useState(String(DEFAULT_STEP_MS));
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  useEffect(() => {
    const onExportFseq = () => {
      setOpen(true);
      setStepMs(String(DEFAULT_STEP_MS));
      setError(null);
    };
    window.addEventListener(EXPORT_FSEQ_EVENT, onExportFseq);
    return () => {
      window.removeEventListener(EXPORT_FSEQ_EVENT, onExportFseq);
    };
  }, []);

  const parsedStepMs = Number(stepMs);
  const stepError =
    Number.isInteger(parsedStepMs) && parsedStepMs >= 1 && parsedStepMs <= 255
      ? null
      : "Step ms must be an integer from 1 to 255.";
  const exportDisabled = exporting || stepError !== null;

  async function exportFseq(event: FormEvent) {
    event.preventDefault();
    if (exportDisabled) return;
    setExporting(true);
    setError(null);
    try {
      const store = useAppStore.getState();
      if (store.runtimeState?.activeBuffer?.viewMode === "text") {
        await runRuntimeCommand(() => commands.updateActiveText(store.localText));
      }
      await runRuntimeCommand(() => commands.exportActiveSequenceFseq(parsedStepMs));
      useAppStore.getState().setError(null);
      setOpen(false);
    } catch (caught) {
      const message = errorMessage(caught);
      setError(message);
      useAppStore.getState().setError(message);
    } finally {
      setExporting(false);
    }
  }

  return (
    <AlertDialog.Root open={open} onOpenChange={setOpen}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="dialog-overlay" />
        <AlertDialog.Content className="dialog-content export-fseq-dialog">
          <AlertDialog.Title>Export FSEQ</AlertDialog.Title>
          <form className="export-fseq-form" onSubmit={(event) => void exportFseq(event)}>
            <label>
              <span>Step ms</span>
              <input
                autoFocus
                inputMode="numeric"
                min={1}
                max={255}
                step={1}
                type="number"
                value={stepMs}
                onChange={(event) => {
                  setStepMs(event.target.value);
                  setError(null);
                }}
              />
            </label>
            {(stepError !== null || error !== null) && (
              <div className="new-project-error">{error ?? stepError}</div>
            )}
            <div className="dialog-actions">
              <AlertDialog.Cancel disabled={exporting}>Cancel</AlertDialog.Cancel>
              <button type="submit" disabled={exportDisabled}>
                {exporting ? "Exporting..." : "Export"}
              </button>
            </div>
          </form>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
