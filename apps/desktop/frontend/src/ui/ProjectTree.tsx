import * as AlertDialog from "@radix-ui/react-alert-dialog";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { File, Folder, FolderPlus, Pencil, Plus, Trash2 } from "lucide-react";
import { useMemo, useState, type CSSProperties } from "react";
import type { NodeApi } from "react-arborist";
import { Tree } from "react-arborist";
import { commands } from "../api";
import type { AppSnapshotDto, ProjectDiagnosticDto, WorkspaceEntryDto } from "../bindings";
import { runSnapshotCommand } from "../store";

type TreeNode = {
  id: string;
  name: string;
  kind: WorkspaceEntryDto["kind"];
  hasError: boolean;
  children?: TreeNode[];
};

export function ProjectTree({ snapshot }: { snapshot: AppSnapshotDto }) {
  const treeData = useMemo(
    () => buildTree(snapshot.projectEntries, snapshot.diagnostics, snapshot.projectRoot),
    [snapshot.diagnostics, snapshot.projectEntries, snapshot.projectRoot]
  );
  const [pendingDelete, setPendingDelete] = useState<TreeNode | null>(null);

  return (
    <aside className="project-panel">
      <div className="panel-header">
        <span>Project</span>
        <div className="panel-actions">
          <button aria-label="New file" onClick={() => { createFile(""); }}>
            <Plus size={15} />
          </button>
          <button aria-label="New folder" onClick={() => { createDirectory(""); }}>
            <FolderPlus size={15} />
          </button>
        </div>
      </div>
      <Tree
        data={treeData}
        width="100%"
        height={window.innerHeight - 118}
        indent={18}
        rowHeight={28}
        openByDefault
        onActivate={(node) => {
          if (node.data.kind === "file") {
            void runSnapshotCommand(() => commands.openFile(node.data.id));
          }
        }}
      >
        {(props) => <TreeRow {...props} requestDelete={setPendingDelete} />}
      </Tree>
      <AlertDialog.Root open={pendingDelete !== null} onOpenChange={(open) => { if (!open) setPendingDelete(null); }}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="dialog-overlay" />
          <AlertDialog.Content className="dialog-content">
            <AlertDialog.Title>Delete {pendingDelete?.name}</AlertDialog.Title>
            <AlertDialog.Description>This removes it from the project folder.</AlertDialog.Description>
            <div className="dialog-actions">
              <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
              <AlertDialog.Action
                onClick={() => {
                  if (pendingDelete) void runSnapshotCommand(() => commands.deletePath(pendingDelete.id));
                }}
              >
                Delete
              </AlertDialog.Action>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </aside>
  );
}

function TreeRow({
  node,
  style,
  dragHandle,
  requestDelete
}: {
  node: NodeApi<TreeNode>;
  style: CSSProperties;
  dragHandle?: (el: HTMLDivElement | null) => void;
  requestDelete: (node: TreeNode) => void;
}) {
  const Icon = node.data.kind === "directory" ? Folder : File;
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>
        <div
          ref={dragHandle}
          className={treeRowClassName(node)}
          style={style}
          onDoubleClick={() => {
            if (node.data.kind === "directory") {
              node.toggle();
            }
          }}
        >
          <Icon size={15} />
          <span>{node.data.name}</span>
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu-content">
          {node.data.kind === "directory" && (
            <>
              <ContextMenu.Item className="menu-item" onSelect={() => { createFile(node.data.id); }}>
                <Plus size={14} /> New File
              </ContextMenu.Item>
              <ContextMenu.Item className="menu-item" onSelect={() => { createDirectory(node.data.id); }}>
                <FolderPlus size={14} /> New Folder
              </ContextMenu.Item>
            </>
          )}
          <ContextMenu.Item className="menu-item" onSelect={() => { renameNode(node.data); }}>
            <Pencil size={14} /> Rename
          </ContextMenu.Item>
          <ContextMenu.Item className="menu-item danger" onSelect={() => { requestDelete(node.data); }}>
            <Trash2 size={14} /> Delete
          </ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

function treeRowClassName(node: NodeApi<TreeNode>): string {
  const classes = ["tree-row"];
  if (node.isSelected) classes.push("selected");
  if (node.data.kind === "file" && node.data.hasError) classes.push("file-error");
  return classes.join(" ");
}

function buildTree(
  entries: WorkspaceEntryDto[],
  diagnostics: ProjectDiagnosticDto[],
  projectRoot: string | null
): TreeNode[] {
  const nodes = new Map<string, TreeNode>();
  for (const entry of entries) {
    const node: TreeNode = {
      id: entry.path,
      name: entry.name,
      kind: entry.kind,
      hasError: entry.kind === "file" && hasErrorDiagnostic(entry.path, diagnostics, projectRoot)
    };
    if (entry.kind === "directory") {
      node.children = [];
    }
    nodes.set(entry.path, node);
  }
  const roots: TreeNode[] = [];
  for (const entry of entries) {
    const node = nodes.get(entry.path);
    if (node === undefined) continue;
    const parent = nodes.get(entry.parent);
    if (entry.parent !== "" && parent !== undefined) {
      parent.children?.push(node);
    } else {
      roots.push(node);
    }
  }
  return roots;
}

function hasErrorDiagnostic(
  path: string,
  diagnostics: ProjectDiagnosticDto[],
  projectRoot: string | null
): boolean {
  return diagnostics.some((diagnostic) => diagnostic.severity === "error" && samePath(diagnostic.path, path, projectRoot));
}

function samePath(left: string, right: string, projectRoot: string | null): boolean {
  const normalizedLeft = normalizePath(left);
  const normalizedRight = normalizePath(right);
  if (normalizedLeft === normalizedRight) return true;
  if (projectRoot === null || isAbsolutePath(right)) return false;
  return normalizedLeft === normalizePath(`${projectRoot}/${right}`);
}

function normalizePath(path: string): string {
  return path.replace(/^\/\/\?\//, "").replace(/\\/g, "/").toLowerCase();
}

function isAbsolutePath(path: string): boolean {
  const normalized = normalizePath(path);
  return /^[a-z]:\//.test(normalized) || normalized.startsWith("/");
}

function createFile(parent: string) {
  const name = window.prompt("File name");
  if (name !== null && name !== "") void runSnapshotCommand(() => commands.createFile(parent, name));
}

function createDirectory(parent: string) {
  const name = window.prompt("Folder name");
  if (name !== null && name !== "") void runSnapshotCommand(() => commands.createDirectory(parent, name));
}

function renameNode(node: TreeNode) {
  const newName = window.prompt("New name", node.name);
  if (newName !== null && newName !== "" && newName !== node.name) void runSnapshotCommand(() => commands.renamePath(node.id, newName));
}
