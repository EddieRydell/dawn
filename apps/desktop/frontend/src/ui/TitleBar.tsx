import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Maximize2, Minus, X } from "lucide-react";
import { commands } from "../api";
import { commandRegistry } from "../commandRegistry";
import { useAppStore } from "../store";

const appWindow = getCurrentWindow();

export function TitleBar() {
  return (
    <header className="titlebar" onMouseDown={startTitlebarDrag}>
      <div className="brand">
        Dawn
      </div>
      <nav className="menu-row">
        <Menu label="File" commands={["file.newProject", "file.openProject", "file.save", "file.exportFseq", "file.settings"]} />
        <Menu label="Edit" commands={["file.save"]} />
        <ViewMenu />
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

function ViewMenu() {
  const checked = useAppStore((store) => (store.snapshot?.settings.editorViewMode ?? "gui") === "gui");
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className="menu-trigger">View</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu-content" sideOffset={7}>
          <DropdownMenu.CheckboxItem
            checked={checked}
            className="menu-item"
            onCheckedChange={() => {
              void commandRegistry["view.toggleGuiMode"].run();
            }}
          >
            <span>{commandRegistry["view.toggleGuiMode"].label}</span>
            <span className="shortcut" />
          </DropdownMenu.CheckboxItem>
          {(["view.toggleProjectTree", "project.reload"] as const).map((id) => {
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
