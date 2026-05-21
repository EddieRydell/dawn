import { LayoutDashboard } from "lucide-react";

export function LayoutPanel() {
  return (
    <section className="panel empty-view">
      <LayoutDashboard size={32} />
      <h2>Layout</h2>
      <p>Display and fixture editing will live here.</p>
    </section>
  );
}
