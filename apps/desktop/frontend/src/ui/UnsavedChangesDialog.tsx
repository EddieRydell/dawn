import * as Dialog from "@radix-ui/react-dialog";
import { useTransitionStore } from "../workspaceTransitions";

export function UnsavedChangesDialog() {
  const pending = useTransitionStore((store) => store.pending);
  return (
    <Dialog.Root open={pending !== null} onOpenChange={(open) => { if (!open) pending?.decide("cancel"); }}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content">
          <Dialog.Title>Unsaved project changes</Dialog.Title>
          <Dialog.Description>Save your changes before continuing?</Dialog.Description>
          <ul>{pending?.paths.map((path) => <li key={path}>{path}</li>)}</ul>
          <div className="dialog-actions">
            <button type="button" onClick={() => pending?.decide("cancel")}>Cancel</button>
            <button type="button" onClick={() => pending?.decide("discard")}>Discard</button>
            <button type="button" className="primary" onClick={() => pending?.decide("saveAll")}>Save All</button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
