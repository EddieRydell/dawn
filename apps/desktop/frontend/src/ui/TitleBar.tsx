import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Check, Maximize2, Minus, X } from "lucide-react";
import { commands } from "../api";
import { commandRegistry } from "../commandRegistry";
import { useAppStore } from "../store";
import { setGlobalMarkDisplayMode, useMarkDisplayMode, type MarkDisplayMode } from "./gui/sequence/marks";
import { requestOpenLayerGraph } from "./uiEvents";

const appWindow = getCurrentWindow();

export function TitleBar() {
  return (
    <header className="titlebar" onMouseDown={startTitlebarDrag}>
      <div className="brand">
        Dawn
      </div>
      <nav className="menu-row">
        <Menu
          label="File"
          commands={[
            "file.newProject",
            "file.newSequence",
            "file.openProject",
            "file.save",
            "file.reloadFromDisk",
            "file.exportFseq",
            "file.settings"
          ]}
        />
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
  const showSequenceItems = useAppStore((store) => store.guiDocument?.type === "sequence");
  const [markMode] = useMarkDisplayMode();
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
          {showSequenceItems && (
            <>
              <DropdownMenu.Separator className="menu-separator" />
              <DropdownMenu.Item className="menu-item" onSelect={requestOpenLayerGraph}>
                <span>Layer Graph</span>
                <span className="shortcut" />
              </DropdownMenu.Item>
              <DropdownMenu.Separator className="menu-separator" />
              <DropdownMenu.Label className="menu-label">Mark display</DropdownMenu.Label>
              <DropdownMenu.RadioGroup
                value={markMode}
                onValueChange={(value) => {
                  setGlobalMarkDisplayMode(value as MarkDisplayMode);
                }}
              >
                <MarkDisplayItem value="overlay" label="Overlay" />
                <MarkDisplayItem value="strip" label="Strip" />
                <MarkDisplayItem value="hidden" label="Hidden" />
              </DropdownMenu.RadioGroup>
            </>
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function MarkDisplayItem({ value, label }: { value: MarkDisplayMode; label: string }) {
  return (
    <DropdownMenu.RadioItem className="menu-item" value={value}>
      <span>{label}</span>
      <DropdownMenu.ItemIndicator>
        <Check size={13} />
      </DropdownMenu.ItemIndicator>
    </DropdownMenu.RadioItem>
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
