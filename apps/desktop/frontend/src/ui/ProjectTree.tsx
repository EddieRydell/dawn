import * as AlertDialog from "@radix-ui/react-alert-dialog";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { ChevronDown, ChevronRight, ExternalLink, File, Folder, FolderPlus, GitFork, Pencil, Plus, RefreshCw, Trash2 } from "lucide-react";
import { forwardRef, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type HTMLAttributes, type RefObject } from "react";
import type { NodeApi } from "react-arborist";
import { ListOuterElement, Tree } from "react-arborist";
import { commands } from "../api";
import type { AppSnapshot, PackageDependencyStatus, PackageStatus, ProjectDiagnostic, WorkspaceEntry, WorkspaceLayoutState } from "../types";
import { runSnapshotCommand } from "../store";
import { THEME_LAYOUT, THEME_METRICS } from "../theme";

type TreeNode = {
  id: string;
  name: string;
  kind: WorkspaceEntry["kind"];
  hasError: boolean;
  children?: TreeNode[];
};

type ProjectFileKind = "default" | "project" | "setup" | "layout" | "fixture" | "patch" | "curve" | "gradient" | "effect" | "sequence" | "operator";

export function ProjectTree({ snapshot, workspaceLayout, onWorkspaceLayoutChange }: { snapshot: AppSnapshot; workspaceLayout: WorkspaceLayoutState; onWorkspaceLayoutChange: (layout: WorkspaceLayoutState) => void }) {
  const treeData = useMemo(
    () => buildTree(snapshot.projectEntries, snapshot.diagnostics, snapshot.projectRoot),
    [snapshot.diagnostics, snapshot.projectEntries, snapshot.projectRoot]
  );
  const [pendingDelete, setPendingDelete] = useState<TreeNode | null>(null);
  const treeShellRef = useRef<HTMLDivElement | null>(null);
  const treeRef = useRef<HTMLDivElement | null>(null);
  const railRef = useRef<HTMLDivElement | null>(null);
  const packagePanelRef = useRef<HTMLElement | null>(null);
  const dragRef = useRef<{ pointerId: number; startY: number; startScrollTop: number } | null>(null);
  const workspaceLayoutRef = useRef(workspaceLayout);
  const [scrollbar, setScrollbar] = useState({ top: 0, height: 0, scrollable: false });
  const [packagePanelHeight, setPackagePanelHeight] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(() => window.innerHeight);

  useEffect(() => {
    workspaceLayoutRef.current = workspaceLayout;
  }, [workspaceLayout]);

  useEffect(() => {
    const panel = packagePanelRef.current;
    if (panel === null) return;
    const updatePackagePanelHeight = () => {
      setPackagePanelHeight(panel.getBoundingClientRect().height);
    };
    updatePackagePanelHeight();
    const resizeObserver = new ResizeObserver(updatePackagePanelHeight);
    resizeObserver.observe(panel);
    return () => {
      resizeObserver.disconnect();
    };
  }, []);

  useEffect(() => {
    const updateViewportHeight = () => {
      setViewportHeight(window.innerHeight);
    };
    window.addEventListener("resize", updateViewportHeight);
    return () => {
      window.removeEventListener("resize", updateViewportHeight);
    };
  }, []);

  const updateScrollbar = useCallback(() => {
    const tree = treeRef.current ?? treeShellRef.current?.querySelector<HTMLDivElement>(".project-tree-scroll-content") ?? null;
    if (tree === null) return;
    treeRef.current = tree;
    const scrollable = tree.scrollHeight > tree.clientHeight + 1;
    const railHeight = Math.max(1, tree.clientHeight);
    const height = scrollable ? Math.max(THEME_METRICS.scrollbarThumbMinHeight, (tree.clientHeight / tree.scrollHeight) * railHeight) : railHeight;
    const maxTop = Math.max(0, railHeight - height);
    const top = scrollable ? (tree.scrollTop / Math.max(1, tree.scrollHeight - tree.clientHeight)) * maxTop : 0;
    setScrollbar((current) =>
      current.top === top && current.height === height && current.scrollable === scrollable
        ? current
        : { top, height, scrollable }
    );
  }, []);

  useEffect(() => {
    const tree = treeRef.current ?? treeShellRef.current?.querySelector<HTMLDivElement>(".project-tree-scroll-content") ?? null;
    if (tree === null) return;
    treeRef.current = tree;
    updateScrollbar();
    const resizeObserver = new ResizeObserver(updateScrollbar);
    resizeObserver.observe(tree);
    tree.addEventListener("scroll", updateScrollbar, { passive: true });
    return () => {
      resizeObserver.disconnect();
      tree.removeEventListener("scroll", updateScrollbar);
    };
  }, [treeData, updateScrollbar]);

  const scrollToPointer = useCallback((clientY: number) => {
    const tree = treeRef.current;
    const rail = railRef.current;
    if (tree === null || rail === null || !scrollbar.scrollable) return;
    const railRect = rail.getBoundingClientRect();
    const maxTop = Math.max(1, railRect.height - scrollbar.height);
    const top = Math.max(0, Math.min(maxTop, clientY - railRect.top - scrollbar.height / 2));
    tree.scrollTop = (top / maxTop) * Math.max(1, tree.scrollHeight - tree.clientHeight);
  }, [scrollbar.height, scrollbar.scrollable]);

  return (
    <aside className="project-panel">
      <div className="panel-header">
        <span>Project</span>
        <div className="panel-actions">
          <button aria-label="New file" onClick={() => { createFile(""); }}>
          <Plus size={THEME_METRICS.iconSizeCompact} />
          </button>
          <button aria-label="New folder" onClick={() => { createDirectory(""); }}>
          <FolderPlus size={THEME_METRICS.iconSizeCompact} />
          </button>
        </div>
      </div>
      <PackagePanel snapshot={snapshot} panelRef={packagePanelRef} />
      <div ref={treeShellRef} className="project-tree-shell">
        <Tree
          data={treeData}
          width={THEME_LAYOUT.projectTreeWidth}
          height={Math.max(
            THEME_METRICS.projectTreeRowHeight,
            viewportHeight - THEME_METRICS.projectTreeViewportInset - packagePanelHeight
          )}
          indent={THEME_METRICS.projectTreeIndent}
          rowHeight={THEME_METRICS.projectTreeRowHeight}
          openByDefault={workspaceLayout.projectTreeExpandedPaths === null || workspaceLayout.projectTreeExpandedPaths === undefined}
          initialOpenState={Object.fromEntries((workspaceLayout.projectTreeExpandedPaths ?? []).map((path) => [path, true]))}
          outerElementType={ProjectTreeScrollContent}
          onScroll={updateScrollbar}
          onToggle={(id) => {
            const currentLayout = workspaceLayoutRef.current;
            const expandedPaths = new Set(currentLayout.projectTreeExpandedPaths ?? treeDirectoryPaths(treeData));
            if (expandedPaths.has(id)) expandedPaths.delete(id);
            else expandedPaths.add(id);
            const nextLayout = { ...currentLayout, projectTreeExpandedPaths: [...expandedPaths].sort() };
            workspaceLayoutRef.current = nextLayout;
            onWorkspaceLayoutChange(nextLayout);
            window.requestAnimationFrame(updateScrollbar);
          }}
          onActivate={(node) => {
            if (node.data.kind === "file") {
              void runSnapshotCommand(() => commands.openFile(node.data.id));
            }
          }}
        >
          {(props) => <TreeRow {...props} requestDelete={setPendingDelete} />}
        </Tree>
        <div className="editor-scrollbar" aria-hidden={!scrollbar.scrollable}>
          <div
            ref={railRef}
            className="editor-scrollbar-rail"
            onPointerDown={(event) => {
              if (!scrollbar.scrollable) return;
              event.currentTarget.setPointerCapture(event.pointerId);
              scrollToPointer(event.clientY);
            }}
          >
            <div
              className={`editor-scrollbar-thumb ${scrollbar.scrollable ? "" : "disabled"}`}
              style={{ top: `${scrollbar.top}px`, height: `${scrollbar.height}px` }}
              onPointerDown={(event) => {
                if (!scrollbar.scrollable) return;
                event.stopPropagation();
                event.currentTarget.setPointerCapture(event.pointerId);
                dragRef.current = {
                  pointerId: event.pointerId,
                  startY: event.clientY,
                  startScrollTop: treeRef.current?.scrollTop ?? 0
                };
              }}
              onPointerMove={(event) => {
                const drag = dragRef.current;
                const tree = treeRef.current;
                const rail = railRef.current;
                if (drag === null || tree === null || rail === null || drag.pointerId !== event.pointerId) return;
                const maxTop = Math.max(1, rail.clientHeight - scrollbar.height);
                const scrollMax = Math.max(1, tree.scrollHeight - tree.clientHeight);
                tree.scrollTop = drag.startScrollTop + ((event.clientY - drag.startY) / maxTop) * scrollMax;
              }}
              onPointerUp={(event) => {
                if (dragRef.current?.pointerId === event.pointerId) dragRef.current = null;
              }}
              onPointerCancel={(event) => {
                if (dragRef.current?.pointerId === event.pointerId) dragRef.current = null;
              }}
            />
          </div>
        </div>
      </div>
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

function PackagePanel({ snapshot, panelRef }: { snapshot: AppSnapshot; panelRef: RefObject<HTMLElement | null> }) {
  const packageStatus = snapshot.package;
  const [pendingRemoval, setPendingRemoval] = useState<PackageDependencyStatus | null>(null);
  const hasRegistryDependencies = packageStatus.dependencies.some((dependency) => dependency.source === "registry");
  const hasAvailableUpdates = packageStatus.dependencies.some((dependency) => dependency.updateAvailable === true);
  const canResolve = packageStatus.root !== null && packageStatus.manifestValid && packageStatus.lockCurrent;
  const canUpdateAll = canResolve && hasRegistryDependencies && (!packageStatus.updateChecked || hasAvailableUpdates);

  return (
    <section ref={panelRef} className="package-panel" aria-label="Packages">
      <div className="package-panel-heading">
        <span>Packages</span>
        <div className="package-panel-actions">
          <button
            type="button"
            disabled={packageStatus.root === null || !packageStatus.manifestValid}
            onClick={() => void runSnapshotCommand(commands.syncPackages)}
          >
            <RefreshCw size={THEME_METRICS.iconSizeExtraSmall} />
            Sync
          </button>
          <button
            type="button"
            disabled={!canResolve || !hasRegistryDependencies}
            onClick={() => void runSnapshotCommand(commands.checkPackageUpdates)}
          >
            Check
          </button>
          <button
            type="button"
            disabled={!canUpdateAll}
            onClick={() => void runSnapshotCommand(() => commands.updatePackages(null))}
          >
            Update all
          </button>
        </div>
      </div>
      <div className="package-panel-status">
        <span className={packageReadinessClassName(packageStatus)}>
          {packageReadinessLabel(packageStatus)}
        </span>
        {packageStatus.registry !== null ? <small title={packageStatus.registry}>{packageStatus.registry}</small> : null}
      </div>
      {packageStatus.message !== null && packageStatus.message !== "" ? (
        <p className="package-panel-message" role="status">{packageStatus.message}</p>
      ) : null}
      {packageStatus.dependencies.length > 0 ? (
        <ul className="package-dependency-list">
          {packageStatus.dependencies.map((dependency) => {
            const registryDependency = dependency.source === "registry";
            return (
              <li key={dependency.alias} className="package-dependency-card">
                <div className="package-dependency-summary">
                  <div className="package-dependency-name">
                    <span>{dependency.alias}</span>
                    <small title={dependency.package ?? dependency.requirement}>
                      {dependency.package ?? dependency.requirement}
                    </small>
                  </div>
                  <div className="package-dependency-version">
                    <span>{dependency.lockedVersion ?? (registryDependency ? "Unresolved" : "Editable")}</span>
                    <small>{registryDependency ? dependency.requirement : "Path dependency"}</small>
                  </div>
                </div>
                <div className="package-dependency-state">
                  <span className={packageCacheClassName(dependency)}>{packageCacheLabel(dependency)}</span>
                  {dependency.updateAvailable === true ? <span className="status-warning">Update available</span> : null}
                  {packageStatus.updateChecked && dependency.updateAvailable === false ? <span className="status-ok">Current</span> : null}
                </div>
                {dependency.warnings.length > 0 ? (
                  <ul className="package-warning-list">
                    {dependency.warnings.map((warning) => <li key={warning}>{warning}</li>)}
                  </ul>
                ) : null}
                <div className="package-dependency-actions">
                  {registryDependency ? (
                    <>
                      <button
                        type="button"
                        disabled={!canResolve}
                        onClick={() => void runSnapshotCommand(() => commands.updatePackages(dependency.alias))}
                      >
                        Update
                      </button>
                      <button
                        type="button"
                        disabled={!canResolve || dependency.cache !== "ready"}
                        onClick={() => void runSnapshotCommand(() => commands.forkPackageDependency(dependency.alias))}
                      >
                        <GitFork size={THEME_METRICS.iconSizeExtraSmall} />
                        Fork
                      </button>
                    </>
                  ) : null}
                  {dependency.websiteUrl !== null ? (
                    <button
                      type="button"
                      title={`Open ${dependency.package ?? dependency.alias} on the registry website`}
                      onClick={() => void runSnapshotCommand(() => commands.openPackagePage(dependency.alias))}
                    >
                      <ExternalLink size={THEME_METRICS.iconSizeExtraSmall} />
                      Website
                    </button>
                  ) : null}
                  <button
                    type="button"
                    className="package-danger-action"
                    disabled={packageStatus.root === null || !packageStatus.manifestValid}
                    onClick={() => { setPendingRemoval(dependency); }}
                  >
                    <Trash2 size={THEME_METRICS.iconSizeExtraSmall} />
                    Remove
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      ) : (
        <span className="package-panel-empty">No dependencies</span>
      )}
      {packageStatus.warnings.length > 0 ? (
        <ul className="package-compatibility-list" aria-label="Package compatibility warnings">
          {packageStatus.warnings.map((warning) => (
            <li key={`${warning.package}:${warning.message}`} className={warning.breaking ? "breaking" : ""}>
              <span>{warning.package}</span>
              <small>{warning.message}</small>
            </li>
          ))}
        </ul>
      ) : null}
      {packageStatus.modules.length > 0 ? (
        <details className="package-module-tree">
          <summary>
            <span>Dependencies</span>
            <small>{packageStatus.modules.length}</small>
          </summary>
          <ul>
            {packageStatus.modules.map((module) => (
              <li key={module.moduleId}>
                <details>
                  <summary>
                    <span>{module.identity}</span>
                    <small>{module.version ?? "editable"}</small>
                  </summary>
                  <ul className="package-document-list">
                    {module.documents.map((document) => (
                      <li key={document}>
                        <File size={THEME_METRICS.iconSizeExtraSmall} />
                        <span>{document}</span>
                      </li>
                    ))}
                  </ul>
                </details>
              </li>
            ))}
          </ul>
        </details>
      ) : null}
      <AlertDialog.Root open={pendingRemoval !== null} onOpenChange={(open) => { if (!open) setPendingRemoval(null); }}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="dialog-overlay" />
          <AlertDialog.Content className="dialog-content">
            <AlertDialog.Title>Remove {pendingRemoval?.alias}?</AlertDialog.Title>
            <AlertDialog.Description>
              This removes the dependency from dawn-package.json and validates the remaining project before accepting the change.
            </AlertDialog.Description>
            <div className="dialog-actions">
              <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
              <AlertDialog.Action
                onClick={() => {
                  const dependency = pendingRemoval;
                  setPendingRemoval(null);
                  if (dependency !== null) {
                    void runSnapshotCommand(() => commands.removePackageDependency(dependency.alias));
                  }
                }}
              >
                Remove
              </AlertDialog.Action>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </section>
  );
}

function packageReadinessLabel(status: PackageStatus): string {
  if (status.root === null) return "No package project";
  if (!status.manifestValid) return "Manifest invalid";
  if (!status.lockPresent) return "Lock missing";
  if (!status.lockCurrent) return "Sync required";
  if (status.dependencies.some((dependency) => dependency.cache === "missing")) return "Cache incomplete";
  if (status.dependencies.some((dependency) => dependency.cache === "error")) return "Cache error";
  return "Ready";
}

function packageReadinessClassName(status: PackageStatus): string {
  return packageReadinessLabel(status) === "Ready" ? "status-ok" : "status-warning";
}

function packageCacheLabel(dependency: PackageDependencyStatus): string {
  switch (dependency.cache) {
    case "ready": return "Cached";
    case "missing": return "Cache missing";
    case "local": return "Local";
    case "error": return "Cache error";
    case "unknown": return "Cache unknown";
  }
}

function packageCacheClassName(dependency: PackageDependencyStatus): string {
  return dependency.cache === "ready" || dependency.cache === "local"
    ? "status-ok"
    : dependency.cache === "unknown"
      ? "package-status-muted"
      : "status-warning";
}

function treeDirectoryPaths(nodes: TreeNode[]): string[] {
  const paths: string[] = [];
  for (const node of nodes) {
    if (node.kind !== "directory") continue;
    paths.push(node.id, ...treeDirectoryPaths(node.children ?? []));
  }
  return paths;
}

const ProjectTreeScrollContent = forwardRef<HTMLDivElement, HTMLAttributes<HTMLDivElement>>(function ProjectTreeScrollContent(
  { className, style, ...props },
  ref
) {
  return <ListOuterElement ref={ref} className={`project-tree-scroll-content ${className ?? ""}`} style={style} {...props} />;
});

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
          onClick={() => {
            if (node.data.kind === "directory" && node.data.children?.length !== 0) {
              node.toggle();
            }
          }}
        >
          <span className="tree-row-chevron" aria-hidden="true">
            {node.data.kind === "directory" && node.data.children?.length !== 0
              ? node.isOpen
                ? <ChevronDown size={THEME_METRICS.iconSizeExtraSmall} />
                : <ChevronRight size={THEME_METRICS.iconSizeExtraSmall} />
              : null}
          </span>
          <Icon size={THEME_METRICS.iconSizeCompact} />
          <span>{node.data.name}</span>
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu-content">
          {node.data.kind === "directory" && (
            <>
              <ContextMenu.Item className="menu-item" onSelect={() => { createFile(node.data.id); }}>
                <Plus size={THEME_METRICS.iconSizeSmall} /> New File
              </ContextMenu.Item>
              <ContextMenu.Item className="menu-item" onSelect={() => { createDirectory(node.data.id); }}>
                <FolderPlus size={THEME_METRICS.iconSizeSmall} /> New Folder
              </ContextMenu.Item>
            </>
          )}
          <ContextMenu.Item className="menu-item" onSelect={() => { renameNode(node.data); }}>
            <Pencil size={THEME_METRICS.iconSizeSmall} /> Rename
          </ContextMenu.Item>
          <ContextMenu.Item className="menu-item danger" onSelect={() => { requestDelete(node.data); }}>
            <Trash2 size={THEME_METRICS.iconSizeSmall} /> Delete
          </ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

function treeRowClassName(node: NodeApi<TreeNode>): string {
  const classes = ["tree-row"];
  if (node.data.kind === "file") classes.push(`file-kind-${projectFileKind(node.data.name)}`);
  if (node.isSelected) classes.push("selected");
  if (node.data.kind === "file" && node.data.hasError) classes.push("file-error");
  return classes.join(" ");
}

function projectFileKind(name: string): ProjectFileKind {
  if (name === "project.dawn") return "project";
  const match = /\.([a-z]+)\.dawn$/i.exec(name);
  const suffix = match?.[1]?.toLowerCase();
  switch (suffix) {
    case "setup":
    case "layout":
    case "fixture":
    case "patch":
    case "curve":
    case "gradient":
    case "effect":
    case "sequence":
    case "operator":
      return suffix;
    case undefined:
    default:
      return "default";
  }
}

function buildTree(
  entries: WorkspaceEntry[],
  diagnostics: ProjectDiagnostic[],
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
  diagnostics: ProjectDiagnostic[],
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
