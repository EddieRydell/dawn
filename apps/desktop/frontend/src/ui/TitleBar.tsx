import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Maximize2, Minus, X } from "lucide-react";
import { commandRegistry } from "../commandRegistry";

const appWindow = getCurrentWindow();

export function TitleBar() {
  return (
    <header className="titlebar">
      <div className="titlebar-drag-surface" onMouseDown={startTitlebarDrag} />
      <div className="brand" onMouseDown={startTitlebarDrag}>
        Dawn
      </div>
      <nav className="menu-row">
        <Menu label="File" commands={["file.openProject", "file.save"]} />
        <Menu label="Edit" commands={["file.save"]} />
        <Menu label="View" commands={["view.toggleProjectTree", "project.reload"]} />
      </nav>
      <div className="window-controls">
        <button onClick={() => void appWindow.minimize()} aria-label="Minimize">
          <Minus size={15} />
        </button>
        <button onClick={() => void appWindow.toggleMaximize()} aria-label="Maximize">
          <Maximize2 size={14} />
        </button>
        <button className="close" onClick={() => void appWindow.close()} aria-label="Close">
          <X size={15} />
        </button>
      </div>
    </header>
  );
}

function startTitlebarDrag(event: React.MouseEvent<HTMLElement>) {
  if (event.button !== 0) return;
  event.preventDefault();
  void appWindow.startDragging();
}

function Menu({ label, commands }: { label: string; commands: Array<keyof typeof commandRegistry> }) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className="menu-trigger">{label}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu-content" sideOffset={7}>
          {commands.map((id) => {
            const command = commandRegistry[id];
            return (
              <DropdownMenu.Item
                key={id}
                className="menu-item"
                onSelect={() => {
                  void command.run();
                }}
              >
                <span>{command.label}</span>
                <span className="shortcut">{command.shortcut}</span>
              </DropdownMenu.Item>
            );
          })}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
