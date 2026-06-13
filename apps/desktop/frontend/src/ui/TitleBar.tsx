import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Maximize2, Minus, X } from "lucide-react";
import { commands } from "../api";
import { commandRegistry } from "../commandRegistry";

const appWindow = getCurrentWindow();

export function TitleBar() {
  return (
    <header className="titlebar" onMouseDown={startTitlebarDrag}>
      <div className="brand">
        Dawn
      </div>
      <nav className="menu-row">
        <Menu label="File" commands={["file.newProject", "file.openProject", "file.save", "file.exportFseq"]} />
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
        <button className="close" onClick={() => void closeMainWindow()} aria-label="Close">
          <X size={15} />
        </button>
      </div>
    </header>
  );
}

async function closeMainWindow() {
  await commands.persistAppClose();
  await appWindow.close();
}

function startTitlebarDrag(event: React.MouseEvent<HTMLElement>) {
  if (event.button !== 0) return;
  if (event.target instanceof Element && event.target.closest("button, [role='menuitem'], [role='menu'], [data-radix-popper-content-wrapper]")) return;
  event.preventDefault();
  if (event.detail === 2) {
    void appWindow.toggleMaximize();
    return;
  }
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
