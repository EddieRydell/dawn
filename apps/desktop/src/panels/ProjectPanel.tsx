import { ChevronDown, ChevronRight, FileCode, Folder } from "lucide-react";
import {
  type KeyboardEvent,
  type PointerEvent,
  type RefObject,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState
} from "react";
import { Tree, type MoveHandler, type NodeRendererProps, type RenameHandler, type RowRendererProps } from "react-arborist";
import { useWorkbench } from "../store/workbenchStore";
import type { ProjectEntry } from "../types";

type FileTreeNode = {
  id: string;
  name: string;
  path: string;
  kind: "directory" | "file";
  children?: FileTreeNode[];
};

type DragState = {
  node: FileTreeNode;
  startX: number;
  startY: number;
  dragging: boolean;
};

let suppressNextTreeClick = false;

export function ProjectPanel() {
  const projectState = useWorkbench((state) => state.projectState);
  const activeFile = useWorkbench((state) => state.activeFile);
  const openFile = useWorkbench((state) => state.openFile);
  const renamePath = useWorkbench((state) => state.renamePath);
  const movePaths = useWorkbench((state) => state.movePaths);
  const setStatus = useWorkbench((state) => state.setStatus);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const dragState = useRef<DragState | null>(null);
  const [dragPreview, setDragPreview] = useState<{ label: string; x: number; y: number } | null>(null);
  const size = useElementSize(containerRef);

  const treeData = useMemo(
    () => buildTreeData(projectState?.root, projectState?.entries, projectState?.files ?? []),
    [projectState]
  );

  const onRename: RenameHandler<FileTreeNode> = useCallback(async ({ name, node }) => {
    if (name === node.data.name) return;
    await renamePath(node.data.path, name);
  }, [renamePath]);

  const onMove: MoveHandler<FileTreeNode> = useCallback(async ({ dragNodes, parentNode }) => {
    if (!projectState?.root) return;
    const newParent = parentNode?.data.path ?? projectState.root;
    await movePaths(
      dragNodes.filter((node) => !node.isRoot).map((node) => node.data.path),
      newParent
    );
  }, [movePaths, projectState?.root]);

  const startPointerDrag = useCallback((event: PointerEvent<HTMLElement>, node: FileTreeNode) => {
    if (event.button !== 0) return;
    if ((event.target as HTMLElement).closest("button, input")) return;
    dragState.current = {
      node,
      startX: event.clientX,
      startY: event.clientY,
      dragging: false
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  }, []);

  const updatePointerDrag = useCallback((event: PointerEvent<HTMLElement>) => {
    const current = dragState.current;
    if (!current) return;

    const moved = Math.hypot(event.clientX - current.startX, event.clientY - current.startY);
    if (!current.dragging && moved < 4) return;

    current.dragging = true;
    suppressNextTreeClick = true;
    setDragPreview({ label: current.node.name, x: event.clientX, y: event.clientY });
    event.preventDefault();
  }, []);

  const finishPointerDrag = useCallback(async (event: PointerEvent<HTMLElement>) => {
    const current = dragState.current;
    dragState.current = null;
    setDragPreview(null);
    if (!current?.dragging || !projectState?.root) return;

    const targetRow = document
      .elementFromPoint(event.clientX, event.clientY)
      ?.closest<HTMLElement>("[data-tree-path]");
    const targetPath = targetRow?.dataset.treePath;
    const targetKind = targetRow?.dataset.treeKind;
    const newParent = targetKind === "directory" && targetPath
      ? targetPath
      : targetPath
        ? parentPath(targetPath)
        : projectState.root;

    if (!newParent || parentPath(current.node.path) === newParent) return;
    await movePaths([current.node.path], newParent);
  }, [movePaths, projectState?.root]);

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
            renderRow={FileTreeRowContainer}
            onActivate={(node) => {
              if (node.data.kind === "file") {
                void openFile(node.data.path);
              }
            }}
            onRename={onRename}
            onMove={onMove}
            disableDrag
            disableDrop={({ parentNode }) => Boolean(parentNode && parentNode.data.kind !== "directory")}
            disableEdit={(node) => node.kind === "file" && !isEditableSource(node.path)}
          >
            {(props) => (
              <FileTreeRow
                {...props}
                onPointerDown={startPointerDrag}
                onPointerMove={updatePointerDrag}
                onPointerUp={finishPointerDrag}
                onPointerCancel={() => {
                  dragState.current = null;
                  setDragPreview(null);
                }}
              />
            )}
          </Tree>
        ) : null}
        {treeData.length > 0 ? (
          <div className="tree-hint" onDoubleClick={() => setStatus("Use F2 to rename; drag files or folders to move them.")}>
            F2 rename - drag to move
          </div>
        ) : null}
        {dragPreview ? (
          <div className="tree-drag-layer">
            <div className="tree-drag-preview" style={{ transform: `translate(${dragPreview.x + 8}px, ${dragPreview.y + 8}px)` }}>
              {dragPreview.label}
            </div>
          </div>
        ) : null}
      </div>
    </section>
  );
}

function FileTreeRowContainer({ node, attrs, innerRef, children }: RowRendererProps<FileTreeNode>) {
  return (
    <div
      {...attrs}
      ref={innerRef}
      onFocus={(event) => event.stopPropagation()}
      onClick={(event) => {
        if (suppressNextTreeClick) {
          suppressNextTreeClick = false;
          event.preventDefault();
          event.stopPropagation();
          return;
        }
        node.handleClick(event);
      }}
    >
      {children}
    </div>
  );
}

function FileTreeRow({
  node,
  style,
  dragHandle,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onPointerCancel
}: NodeRendererProps<FileTreeNode> & {
  onPointerDown: (event: PointerEvent<HTMLElement>, node: FileTreeNode) => void;
  onPointerMove: (event: PointerEvent<HTMLElement>) => void;
  onPointerUp: (event: PointerEvent<HTMLElement>) => void;
  onPointerCancel: () => void;
}) {
  const isDirectory = node.data.kind === "directory";

  return (
    <div
      ref={dragHandle}
      data-tree-kind={node.data.kind}
      data-tree-path={node.data.path}
      className={[
        "tree-row",
        node.isSelected ? "selected" : "",
        node.isFocused ? "focused" : "",
        node.isDragging ? "dragging" : "",
        node.willReceiveDrop ? "drop-target" : ""
      ].join(" ")}
      style={style}
      onPointerDown={(event) => onPointerDown(event, node.data)}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
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

function buildTreeData(root: string | undefined, entries: ProjectEntry[] | undefined, files: string[]): FileTreeNode[] {
  const rootNode: FileTreeNode = { id: root ?? "project", name: root ? leafName(root) : "Project", path: root ?? "", kind: "directory", children: [] };
  const projectEntries = entries ?? files.map((path) => ({ path, kind: "file" as const }));

  for (const entry of projectEntries) {
    const relative = root ? entry.path.replace(`${root}\\`, "").replace(`${root}/`, "") : entry.path;
    const parts = relative.split(/[\\/]/).filter(Boolean);
    let cursor = rootNode;

    parts.forEach((part, index) => {
      const isEntry = index === parts.length - 1;
      const path = isEntry ? entry.path : joinTreePath(cursor.path, part);
      const kind: FileTreeNode["kind"] = isEntry ? entry.kind : "directory";
      let child = cursor.children?.find((node) => node.name === part && node.kind === kind);

      if (!child) {
        child = { id: path, name: part, path, kind, children: kind === "file" ? undefined : [] };
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

function parentPath(path: string) {
  const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return index > 0 ? path.slice(0, index) : "";
}

function isEditableSource(path: string) {
  return path.endsWith(".donder");
}

function useElementSize(ref: RefObject<HTMLElement>) {
  const [size, setSize] = useState({ width: 280, height: 0 });

  useEffect(() => {
    const element = ref.current;
    if (!element) return;

    const update = () => {
      const rect = element.getBoundingClientRect();
      const next = { width: Math.max(1, rect.width), height: Math.max(0, rect.height - 20) };
      setSize((current) => (current.width === next.width && current.height === next.height ? current : next));
    };
    update();

    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);

  return size;
}
