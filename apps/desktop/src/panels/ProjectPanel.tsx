import { ChevronDown, ChevronRight, FileCode, Folder } from "lucide-react";
import { type KeyboardEvent, type RefObject, useEffect, useMemo, useRef, useState } from "react";
import { Tree, type MoveHandler, type NodeRendererProps, type RenameHandler } from "react-arborist";
import { useWorkbench } from "../store/workbenchStore";

type FileTreeNode = {
  id: string;
  name: string;
  path: string;
  kind: "directory" | "file";
  children?: FileTreeNode[];
};

export function ProjectPanel() {
  const projectState = useWorkbench((state) => state.projectState);
  const activeFile = useWorkbench((state) => state.activeFile);
  const openFile = useWorkbench((state) => state.openFile);
  const renamePath = useWorkbench((state) => state.renamePath);
  const movePaths = useWorkbench((state) => state.movePaths);
  const setStatus = useWorkbench((state) => state.setStatus);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const size = useElementSize(containerRef);

  const treeData = useMemo(() => buildTreeData(projectState?.root, projectState?.files ?? []), [projectState]);

  const onRename: RenameHandler<FileTreeNode> = async ({ name, node }) => {
    if (name === node.data.name) return;
    await renamePath(node.data.path, name);
  };

  const onMove: MoveHandler<FileTreeNode> = async ({ dragNodes, parentNode }) => {
    if (!projectState?.root) return;
    const newParent = parentNode?.data.path ?? projectState.root;
    await movePaths(
      dragNodes.filter((node) => !node.isRoot).map((node) => node.data.path),
      newParent
    );
  };

  return (
    <section className="panel project-panel">
      <div className="file-tree" ref={containerRef}>
        {treeData.length === 0 ? (
          <div className="empty-tree">No project open.</div>
        ) : size.height > 0 ? (
          <Tree<FileTreeNode>
            data={treeData}
            width={size.width}
            height={size.height}
            rowHeight={24}
            indent={14}
            openByDefault
            selection={activeFile ?? undefined}
            onActivate={(node) => {
              if (node.data.kind === "file") {
                void openFile(node.data.path);
              }
            }}
            onRename={onRename}
            onMove={onMove}
            disableDrop={({ parentNode }) => Boolean(parentNode && parentNode.data.kind !== "directory")}
            disableEdit={(node) => node.kind === "file" && !isEditableSource(node.path)}
            dndRootElement={document.body}
          >
            {FileTreeRow}
          </Tree>
        ) : null}
        {treeData.length > 0 ? (
          <div className="tree-hint" onDoubleClick={() => setStatus("Use F2 to rename; drag files or folders to move them.")}>
            F2 rename · drag to move
          </div>
        ) : null}
      </div>
    </section>
  );
}

function FileTreeRow({ node, style, dragHandle }: NodeRendererProps<FileTreeNode>) {
  const isDirectory = node.data.kind === "directory";

  return (
    <div
      ref={dragHandle}
      className={[
        "tree-row",
        node.isSelected ? "selected" : "",
        node.isFocused ? "focused" : "",
        node.isDragging ? "dragging" : "",
        node.willReceiveDrop ? "drop-target" : ""
      ].join(" ")}
      style={style}
      onDoubleClick={() => {
        if (isDirectory) node.toggle();
      }}
    >
      <button
        className="tree-twist"
        tabIndex={-1}
        onClick={(event) => {
          event.stopPropagation();
          if (isDirectory) node.toggle();
        }}
      >
        {isDirectory ? node.isOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} /> : null}
      </button>
      <span className="tree-icon">{isDirectory ? <Folder size={14} /> : <FileCode size={14} />}</span>
      {node.isEditing ? <RenameInput node={node} /> : <span className="tree-name">{node.data.name}</span>}
    </div>
  );
}

function RenameInput({ node }: { node: NodeRendererProps<FileTreeNode>["node"] }) {
  function submit(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      node.submit(event.currentTarget.value);
    }
    if (event.key === "Escape") {
      node.reset();
    }
  }

  return (
    <input
      className="tree-rename"
      autoFocus
      defaultValue={node.data.name}
      onFocus={(event) => event.currentTarget.select()}
      onBlur={(event) => node.submit(event.currentTarget.value)}
      onKeyDown={submit}
    />
  );
}

function buildTreeData(root: string | undefined, files: string[]): FileTreeNode[] {
  const rootNode: FileTreeNode = { id: root ?? "project", name: root ? leafName(root) : "Project", path: root ?? "", kind: "directory", children: [] };

  for (const file of files) {
    const relative = root ? file.replace(`${root}\\`, "").replace(`${root}/`, "") : file;
    const parts = relative.split(/[\\/]/).filter(Boolean);
    let cursor = rootNode;

    parts.forEach((part, index) => {
      const isFile = index === parts.length - 1;
      const path = isFile ? file : joinTreePath(cursor.path, part);
      const kind: FileTreeNode["kind"] = isFile ? "file" : "directory";
      let child = cursor.children?.find((node) => node.name === part && node.kind === kind);

      if (!child) {
        child = { id: path, name: part, path, kind, children: isFile ? undefined : [] };
        cursor.children = cursor.children ?? [];
        cursor.children.push(child);
        cursor.children.sort(compareTreeNodes);
      }

      cursor = child;
    });
  }

  return rootNode.children ?? [];
}

function compareTreeNodes(left: FileTreeNode, right: FileTreeNode) {
  if (left.kind !== right.kind) return left.kind === "directory" ? -1 : 1;
  return left.name.localeCompare(right.name);
}

function leafName(path: string) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function joinTreePath(parent: string, child: string) {
  return parent ? `${parent}\\${child}` : child;
}

function isEditableSource(path: string) {
  return path.endsWith(".jsonc") || path.endsWith(".vibe");
}

function useElementSize(ref: RefObject<HTMLElement>) {
  const [size, setSize] = useState({ width: 280, height: 0 });

  useEffect(() => {
    const element = ref.current;
    if (!element) return;

    const update = () => {
      const rect = element.getBoundingClientRect();
      setSize({ width: Math.max(1, rect.width), height: Math.max(0, rect.height - 20) });
    };
    update();

    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);

  return size;
}
