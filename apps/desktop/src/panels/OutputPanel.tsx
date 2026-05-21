import { useWorkbench } from "../store/workbenchStore";

export function OutputPanel() {
  const status = useWorkbench((state) => state.status);

  return (
    <section className="panel output-panel">
      <pre>{status}</pre>
    </section>
  );
}
