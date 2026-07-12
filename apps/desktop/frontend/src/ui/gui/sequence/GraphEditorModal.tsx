import "@xyflow/react/dist/style.css";

import {
  Background,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  NodeResizeControl,
  Position,
  ReactFlow,
  ResizeControlVariant,
  useEdgesState,
  useNodesState,
  type ReactFlowInstance,
  type Connection,
  type Edge,
  type Node,
  type NodeProps,
  type OnSelectionChangeFunc
} from "@xyflow/react";
import { X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type {
  SequenceEditorDocument,
  SequenceEffect,
  SequenceGraphNode,
  SequenceGraphOperator,
  SequenceGraphOperatorDefinition,
  SequenceLayer
} from "../../../types";
import {
  GRAPH_NEUTRAL_EDGE_COLOR,
  graphEdgeId,
  graphEdgeLineages,
  parseGraphEdgeId,
  type GraphEdgeIdParts
} from "./graphEdge";
import { graphOperatorDefinition, graphOperatorKey } from "./graphOperator";
import { TypedParamInput } from "./params/TypedParamInput";

type GraphNodeData = {
  label: string;
  kind: "layer" | "operator" | "output";
  color: string | null;
  layerId: number | null;
  inputs: GraphPort[];
  outputs: GraphPort[];
};

type GraphFlowNode = Node<GraphNodeData, "dawn">;

type GraphPort = {
  id: string;
  label: string;
};

type SelectedGraphItem = { type: "node"; id: string } | { type: "edge"; id: string } | null;

type PendingLayerDeletion = {
  id: number;
  name: string;
  effectCount: number;
  migrateToLayerId: number;
};

type GraphContextMenu =
  | {
      type: "pane";
      screenX: number;
      screenY: number;
      flowX: number;
      flowY: number;
    }
  | {
      type: "edge";
      screenX: number;
      screenY: number;
      edgeId: string;
    }
  | {
      type: "node";
      screenX: number;
      screenY: number;
      nodeId: string;
    }
  | null;

const GRAPH_NODE_TYPES = {
  dawn: GraphFlowNodeView
};

export function GraphEditorModal({
  document,
  selectedItem,
  setSelectedItem,
  onClose
}: {
  document: SequenceEditorDocument;
  selectedItem: SelectedGraphItem;
  setSelectedItem: (item: SelectedGraphItem) => void;
  onClose: () => void;
}) {
  return (
    <div className="graph-modal-backdrop" role="presentation">
      <div className="graph-modal" role="dialog" aria-modal="true" aria-label="Composition graph editor">
        <div className="graph-modal-header">
          <div>
            <h2>Composition Graph</h2>
            <span>Sequence layers</span>
          </div>
          <button type="button" className="icon-button" onClick={onClose} aria-label="Close graph editor">
            <X size={16} />
          </button>
        </div>
        <GraphEditorWorkspace document={document} selectedItem={selectedItem} setSelectedItem={setSelectedItem} />
      </div>
    </div>
  );
}

function GraphEditorWorkspace({
  document,
  selectedItem,
  setSelectedItem
}: {
  document: SequenceEditorDocument;
  selectedItem: SelectedGraphItem;
  setSelectedItem: (item: SelectedGraphItem) => void;
}) {
  const graph = document.compositionGraph;
  const flowNodes = useMemo(
    () => graphFlowNodes(graph.nodes, graph.operatorCatalog),
    [graph.nodes, graph.operatorCatalog]
  );
  const edgeLineages = useMemo(() => graphEdgeLineages(graph), [graph]);
  const selectedOperator = useMemo(() => {
    if (selectedItem?.type !== "node") return null;
    const node = graph.nodes.find((candidate) => candidate.id === selectedItem.id);
    return node?.kind.type === "operator" ? { node, kind: node.kind } : null;
  }, [graph.nodes, selectedItem]);

  const edges = useMemo<Edge[]>(
    () =>
      graph.edges.map((edge) => {
        const id = graphEdgeId(edge);
        const color = edgeLineages.get(id)?.color ?? GRAPH_NEUTRAL_EDGE_COLOR;
        return {
          id,
          source: edge.fromNode,
          target: edge.toNode,
          sourceHandle: edge.fromPort,
          targetHandle: edge.toPort,
          markerEnd: {
            type: MarkerType.ArrowClosed,
            color
          },
          style: {
            stroke: color,
            strokeWidth: 2
          }
        };
      }),
    [edgeLineages, graph.edges]
  );

  const addOperator = (operator: SequenceGraphOperator, x = nextNodeX(graph), y = 280) => {
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "addGraphOperatorNode",
        operator,
        x,
        y
      })
    );
  };
  const addLayerAt = (x: number, y: number) => {
    const layerNumber = document.layers.length + 1;
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "createLayerAt",
        name: `Layer ${layerNumber}`,
        color: layerColor(layerNumber - 1),
        x,
        y
      })
    );
  };

  return (
    <div className="graph-workspace">
      <div className="graph-modal-body">
        <div className="graph-flow-pane">
          <GraphFlowCanvas
            nodes={flowNodes}
            edges={edges}
            selectedItem={selectedItem}
            setSelectedItem={setSelectedItem}
            addOperatorAt={addOperator}
            addLayerAt={addLayerAt}
            operatorCatalog={graph.operatorCatalog}
            layers={document.layers}
            effects={document.effects}
          />
        </div>
        {selectedOperator !== null && (
          <GraphOperatorInspector
            node={selectedOperator.node}
            kind={selectedOperator.kind}
            definition={graphOperatorDefinition(graph.operatorCatalog, selectedOperator.kind.operator)}
            document={document}
          />
        )}
      </div>
    </div>
  );
}

function GraphOperatorInspector({
  node,
  kind,
  definition,
  document
}: {
  node: SequenceGraphNode;
  kind: Extract<SequenceGraphNode["kind"], { type: "operator" }>;
  definition: SequenceGraphOperatorDefinition;
  document: SequenceEditorDocument;
}) {
  return (
    <aside className="graph-operator-inspector">
      <div className="graph-operator-inspector-heading">
        <span>{kind.operator.type === "builtin" ? "Built-in operator" : "Project operator"}</span>
        <h3>{definition.displayName}</h3>
        <code>{definition.sourceName}</code>
      </div>
      {kind.params.length === 0 ? (
        <p className="graph-operator-no-params">No parameters</p>
      ) : (
        <div className="effect-param-section">
          <h3>Parameters</h3>
          {kind.params.map((param, index) => (
            <div
              key={`${node.id}:${param.name}`}
              className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}
            >
              <TypedParamInput
                param={param}
                commitParam={(name, value) =>
                  runGuiEditCommand(() =>
                    commands.applySequenceGuiEdit({
                      type: "updateGraphOperatorParam",
                      nodeId: node.id,
                      name,
                      value
                    })
                  ).then(() => undefined)
                }
                curveLibrary={document.curveLibrary}
                gradientLibrary={document.gradientLibrary}
                markCollections={document.markCollections}
                linkCurve={(name, curve) =>
                  runGuiEditCommand(() =>
                    commands.applySequenceGuiEdit({
                      type: "linkGraphOperatorCurve",
                      nodeId: node.id,
                      name,
                      sourcePath: curve.path,
                      objectKey: curve.objectKey
                    })
                  ).then(() => undefined)
                }
                unlinkCurve={(name) =>
                  runGuiEditCommand(() =>
                    commands.applySequenceGuiEdit({
                      type: "unlinkGraphOperatorCurve",
                      nodeId: node.id,
                      name
                    })
                  ).then(() => undefined)
                }
                linkGradient={(name, gradient) =>
                  runGuiEditCommand(() =>
                    commands.applySequenceGuiEdit({
                      type: "linkGraphOperatorGradient",
                      nodeId: node.id,
                      name,
                      sourcePath: gradient.path,
                      objectKey: gradient.objectKey
                    })
                  ).then(() => undefined)
                }
                unlinkGradient={(name) =>
                  runGuiEditCommand(() =>
                    commands.applySequenceGuiEdit({
                      type: "unlinkGraphOperatorGradient",
                      nodeId: node.id,
                      name
                    })
                  ).then(() => undefined)
                }
              />
            </div>
          ))}
        </div>
      )}
    </aside>
  );
}

function GraphFlowCanvas({
  nodes,
  edges: sourceEdges,
  selectedItem,
  setSelectedItem,
  addOperatorAt,
  addLayerAt,
  operatorCatalog,
  layers,
  effects
}: {
  nodes: GraphFlowNode[];
  edges: Edge[];
  selectedItem: SelectedGraphItem;
  setSelectedItem: (item: SelectedGraphItem) => void;
  addOperatorAt: (operator: SequenceGraphOperator, x: number, y: number) => void;
  addLayerAt: (x: number, y: number) => void;
  operatorCatalog: SequenceGraphOperatorDefinition[];
  layers: SequenceLayer[];
  effects: SequenceEffect[];
}) {
  const [flowNodes, setFlowNodes, onFlowNodesChange] = useNodesState<GraphFlowNode>(nodes);
  const [flowEdges, setFlowEdges, onFlowEdgesChange] = useEdgesState(sourceEdges);
  const [contextMenu, setContextMenu] = useState<GraphContextMenu>(null);
  const [pendingLayerDeletion, setPendingLayerDeletion] = useState<PendingLayerDeletion | null>(null);
  const flow = useRef<ReactFlowInstance<GraphFlowNode> | null>(null);

  useEffect(() => {
    setFlowNodes((current) => mergeGraphNodes(nodes, current));
  }, [nodes, setFlowNodes]);

  useEffect(() => {
    setFlowEdges((current) =>
      sourceEdges.map((sourceEdge) => {
        const currentEdge = current.find((edge) => edge.id === sourceEdge.id);
        if (currentEdge === undefined) return sourceEdge;
        const mergedEdge = { ...sourceEdge };
        if (currentEdge.selected !== undefined) mergedEdge.selected = currentEdge.selected;
        return mergedEdge;
      })
    );
  }, [setFlowEdges, sourceEdges]);

  const closeContextMenu = () => {
    setContextMenu(null);
  };

  const deleteEdgeById = useCallback(
    (edgeId: string) => {
      const edge = flowEdges.find((candidate) => candidate.id === edgeId);
      if (edge === undefined) return;
      void deleteFlowEdge(edge).then(() => {
        setSelectedItem(null);
      });
    },
    [flowEdges, setSelectedItem]
  );

  const deleteNodeById = useCallback(
    (nodeId: string) => {
      const node = flowNodes.find((candidate) => candidate.id === nodeId);
      if (node?.data.kind !== "operator") return;
      void runGuiEditCommand(() =>
        commands.applySequenceGuiEdit({
          type: "deleteGraphNode",
          nodeId
        })
      ).then(() => {
        setSelectedItem(null);
      });
    },
    [flowNodes, setSelectedItem]
  );

  const deleteLayer = useCallback(
    (id: number, migrateToLayerId: number) => {
      void runGuiEditCommand(() =>
        commands.applySequenceGuiEdit({
          type: "deleteLayer",
          id,
          migrateToLayerId
        })
      ).then(() => {
        setPendingLayerDeletion(null);
        setSelectedItem(null);
      });
    },
    [setSelectedItem]
  );

  const deleteGraphItem = useCallback(
    (item: Exclude<SelectedGraphItem, null>) => {
      if (item.type === "edge") {
        deleteEdgeById(item.id);
        return;
      }
      const node = flowNodes.find((candidate) => candidate.id === item.id);
      if (node === undefined || node.data.kind === "output") return;
      if (node.data.kind === "operator") {
        deleteNodeById(item.id);
        return;
      }
      const layerId = node.data.layerId;
      if (layerId === null) return;
      const layer = layers.find((candidate) => candidate.id === layerId);
      if (layer === undefined || layer.isDefault) return;
      const effectCount = effects.filter((effect) => effect.layerId === layerId).length;
      const defaultLayer = layers.find((candidate) => candidate.isDefault);
      if (defaultLayer === undefined) return;
      if (effectCount === 0) {
        deleteLayer(layerId, defaultLayer.id);
        return;
      }
      setPendingLayerDeletion({
        id: layerId,
        name: layer.name,
        effectCount,
        migrateToLayerId: defaultLayer.id
      });
    },
    [deleteEdgeById, deleteLayer, deleteNodeById, effects, flowNodes, layers]
  );

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Delete" && event.key !== "Backspace") return;
      if (
        event.target instanceof HTMLInputElement ||
        event.target instanceof HTMLSelectElement ||
        event.target instanceof HTMLTextAreaElement
      ) {
        return;
      }
      if (selectedItem === null) return;
      event.preventDefault();
      deleteGraphItem(selectedItem);
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [deleteGraphItem, selectedItem]);

  const handleSelectionChange = useCallback<OnSelectionChangeFunc<GraphFlowNode>>(
    ({ nodes: selectedNodes, edges: selectedEdges }) => {
      const node = selectedNodes[0];
      if (node !== undefined) {
        setSelectedItem({ type: "node", id: node.id });
        return;
      }
      const edge = selectedEdges[0];
      setSelectedItem(edge === undefined ? null : { type: "edge", id: edge.id });
    },
    [setSelectedItem]
  );

  return (
    <>
      <ReactFlow
        nodes={flowNodes}
        edges={flowEdges}
        nodeTypes={GRAPH_NODE_TYPES}
        fitView
        minZoom={0.2}
        maxZoom={2}
        deleteKeyCode={null}
        defaultEdgeOptions={{
          type: "default",
          interactionWidth: 32,
          className: "graph-flow-edge"
        }}
        onInit={(instance) => {
          flow.current = instance;
        }}
        onNodesChange={onFlowNodesChange}
        onEdgesChange={onFlowEdgesChange}
        onNodeDragStart={() => {
          closeContextMenu();
        }}
        onNodeDragStop={(_, node) => {
          void persistNodePosition(node);
        }}
        onNodeContextMenu={(event, node) => {
          event.preventDefault();
          event.stopPropagation();
          setSelectedItem({ type: "node", id: node.id });
          setContextMenu({
            type: "node",
            screenX: event.clientX,
            screenY: event.clientY,
            nodeId: node.id
          });
        }}
        onEdgeClick={closeContextMenu}
        onEdgeContextMenu={(event, edge) => {
          event.preventDefault();
          setSelectedItem({ type: "edge", id: edge.id });
          setContextMenu({
            type: "edge",
            screenX: event.clientX,
            screenY: event.clientY,
            edgeId: edge.id
          });
        }}
        onPaneClick={closeContextMenu}
        onPaneContextMenu={(event) => {
          event.preventDefault();
          const position = flow.current?.screenToFlowPosition({ x: event.clientX, y: event.clientY }) ?? { x: 0, y: 0 };
          setContextMenu({
            type: "pane",
            screenX: event.clientX,
            screenY: event.clientY,
            flowX: position.x,
            flowY: position.y
          });
        }}
        onConnect={(connection) => {
          closeContextMenu();
          connectNodes(connection);
        }}
        onEdgesDelete={(deleted) => {
          closeContextMenu();
          disconnectEdges(deleted);
        }}
        onSelectionChange={handleSelectionChange}
      >
        <Background color="#2c3036" gap={32} />
        <MiniMap className="graph-flow-minimap" nodeStrokeWidth={2} pannable zoomable />
        <Controls className="graph-flow-controls" showInteractive={false} />
      </ReactFlow>
      {contextMenu !== null && (
        <div className="graph-context-menu" style={{ left: contextMenu.screenX, top: contextMenu.screenY }}>
          {contextMenu.type === "node" ? (
            <button
              type="button"
              disabled={!graphNodeCanBeDeleted(
                flowNodes.find((node) => node.id === contextMenu.nodeId),
                layers
              )}
              onClick={() => {
                deleteGraphItem({ type: "node", id: contextMenu.nodeId });
                closeContextMenu();
              }}
            >
              Delete node
            </button>
          ) : contextMenu.type === "edge" ? (
            <button
              type="button"
              onClick={() => {
                deleteGraphItem({ type: "edge", id: contextMenu.edgeId });
                closeContextMenu();
              }}
            >
              Delete connection
            </button>
          ) : (
            <>
              <button
                type="button"
                onClick={() => {
                  addLayerAt(contextMenu.flowX, contextMenu.flowY);
                  closeContextMenu();
                }}
              >
                Layer
              </button>
              <div className="graph-context-menu-heading">Built-in operators</div>
              {operatorCatalog.filter((definition) => definition.operator.type === "builtin").map((definition) => (
                <button
                  key={graphOperatorKey(definition.operator)}
                  type="button"
                  onClick={() => {
                    addOperatorAt(definition.operator, contextMenu.flowX, contextMenu.flowY);
                    closeContextMenu();
                  }}
                >
                  {definition.displayName}
                </button>
              ))}
              {operatorCatalog.some((definition) => definition.operator.type === "custom") && (
                <div className="graph-context-menu-heading">Project operators</div>
              )}
              {operatorCatalog.filter((definition) => definition.operator.type === "custom").map((definition) => (
                <button
                  key={graphOperatorKey(definition.operator)}
                  type="button"
                  onClick={() => {
                    addOperatorAt(definition.operator, contextMenu.flowX, contextMenu.flowY);
                    closeContextMenu();
                  }}
                >
                  {definition.displayName}
                </button>
              ))}
            </>
          )}
        </div>
      )}
      {pendingLayerDeletion !== null && (
        <div className="graph-delete-dialog-backdrop" role="presentation">
          <div className="graph-delete-dialog" role="dialog" aria-modal="true" aria-label="Delete layer">
            <h3>Delete {pendingLayerDeletion.name}?</h3>
            <p>
              {pendingLayerDeletion.effectCount}{" "}
              {pendingLayerDeletion.effectCount === 1 ? "effect uses" : "effects use"} this layer.
              Choose where to move them.
            </p>
            <label>
              Move effects to
              <select
                value={pendingLayerDeletion.migrateToLayerId}
                onChange={(event) => {
                  const migrateToLayerId = Number(event.currentTarget.value);
                  setPendingLayerDeletion((current) =>
                    current === null ? null : { ...current, migrateToLayerId }
                  );
                }}
              >
                {layers
                  .filter((layer) => layer.id !== pendingLayerDeletion.id)
                  .map((layer) => (
                    <option key={layer.id} value={layer.id}>
                      {layer.name}
                    </option>
                  ))}
              </select>
            </label>
            <div className="graph-delete-dialog-actions">
              <button
                type="button"
                onClick={() => {
                  setPendingLayerDeletion(null);
                }}
              >
                Cancel
              </button>
              <button
                type="button"
                className="danger-button"
                onClick={() => {
                  deleteLayer(
                    pendingLayerDeletion.id,
                    pendingLayerDeletion.migrateToLayerId
                  );
                }}
              >
                Delete layer
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}

const RESIZE_CORNERS = ["top-left", "top-right", "bottom-left", "bottom-right"] as const;
const RESIZE_EDGES = ["top", "right", "bottom", "left"] as const;

function GraphFlowNodeView({ data }: NodeProps<GraphFlowNode>) {
  return (
    <div
      className={`graph-flow-node-card ${data.kind}`}
      style={data.color === null ? undefined : { borderLeftColor: data.color }}
    >
      {RESIZE_EDGES.map((position) => (
        <NodeResizeControl
          key={position}
          position={position}
          variant={ResizeControlVariant.Line}
          minWidth={140}
          minHeight={80}
          className="graph-flow-resize-edge"
        />
      ))}
      {RESIZE_CORNERS.map((position) => (
        <NodeResizeControl
          key={position}
          position={position}
          minWidth={140}
          minHeight={80}
          className="graph-flow-resize-corner"
        />
      ))}
      <div className="graph-flow-node-title">
        {data.color !== null && <span className="graph-flow-node-swatch" style={{ background: data.color }} />}
        <span>{data.label}</span>
      </div>
      <div className="graph-flow-node-body">
        <div className="graph-flow-port-column inputs">
          {data.inputs.map((port) => (
            <div className="graph-flow-port-row" key={port.id}>
              <Handle
                id={port.id}
                type="target"
                position={Position.Left}
                className="graph-flow-handle input"
                aria-label={`Input ${port.label}`}
                title={port.label}
              />
              <span>{port.label}</span>
            </div>
          ))}
        </div>
        <div className="graph-flow-port-column outputs">
          {data.outputs.map((port) => (
            <div className="graph-flow-port-row" key={port.id}>
              <span>{port.label}</span>
              <Handle
                id={port.id}
                type="source"
                position={Position.Right}
                className="graph-flow-handle output"
                aria-label={`Output ${port.label}`}
                title={port.label}
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function mergeGraphNodes(sourceNodes: GraphFlowNode[], currentNodes: GraphFlowNode[]) {
  const currentById = new Map(currentNodes.map((node) => [node.id, node]));
  return sourceNodes.map((sourceNode) => {
    const currentNode = currentById.get(sourceNode.id);
    if (currentNode === undefined) return sourceNode;
    const mergedNode: GraphFlowNode = {
      ...sourceNode,
      position: currentNode.position
    };
    if (currentNode.selected !== undefined) mergedNode.selected = currentNode.selected;
    if (currentNode.dragging !== undefined) mergedNode.dragging = currentNode.dragging;
    if (currentNode.width !== undefined) mergedNode.width = currentNode.width;
    if (currentNode.height !== undefined) mergedNode.height = currentNode.height;
    return mergedNode;
  });
}

function persistNodePosition(node: GraphFlowNode) {
  return runGuiEditCommand(() =>
    commands.applySequenceGuiEdit({
      type: "moveGraphNode",
      nodeId: node.id,
      x: node.position.x,
      y: node.position.y
    })
  );
}

function graphFlowNodes(
  nodes: SequenceGraphNode[],
  catalog: SequenceGraphOperatorDefinition[]
): GraphFlowNode[] {
  return nodes.map((node) => ({
    id: node.id,
    type: "dawn",
    position: { x: node.x, y: node.y },
    sourcePosition: Position.Right,
    targetPosition: Position.Left,
    initialWidth: 168,
    initialHeight: 96,
    data: {
      label: graphNodeLabel(node, catalog),
      kind: node.kind.type,
      color: graphNodeColor(node),
      layerId: node.kind.type === "layer" ? node.kind.layerId : null,
      inputs: graphNodeInputs(node),
      outputs: graphNodeOutputs(node)
    }
  }));
}

function connectNodes(connection: Connection) {
  const fromNode = connection.source;
  const toNode = connection.target;
  const fromPort = connection.sourceHandle;
  const toPort = connection.targetHandle;
  if (fromPort === null || toPort === null) return;
  void runGuiEditCommand(() =>
    commands.applySequenceGuiEdit({
      type: "connectGraphNodes",
      fromNode,
      fromPort,
      toNode,
      toPort
    })
  );
}

function disconnectEdges(edges: Edge[]) {
  for (const edge of edges) {
    void deleteFlowEdge(edge);
  }
}

function deleteFlowEdge(edge: Edge) {
  const parsed = parseGraphEdgeId(edge.id);
  if (parsed === null) return Promise.resolve();
  return deleteGraphEdge(parsed);
}

function deleteGraphEdge(edge: GraphEdgeIdParts) {
  return runGuiEditCommand(() =>
    commands.applySequenceGuiEdit({
      type: "disconnectGraphNodes",
      fromNode: edge.fromNode,
      fromPort: edge.fromPort,
      toNode: edge.toNode,
      toPort: edge.toPort
    })
  );
}

function graphNodeLabel(node: SequenceGraphNode, catalog: SequenceGraphOperatorDefinition[]) {
  if (node.kind.type === "layer") return node.kind.layerName;
  if (node.kind.type === "operator") {
    return graphOperatorDefinition(catalog, node.kind.operator).displayName;
  }
  return "Output";
}

function graphNodeColor(node: SequenceGraphNode) {
  return node.kind.type === "layer" ? node.kind.layerColor : null;
}

function graphNodeCanBeDeleted(
  node: GraphFlowNode | undefined,
  layers: SequenceLayer[]
) {
  if (node === undefined || node.data.kind === "output") return false;
  if (node.data.kind === "operator") return true;
  const layer = layers.find((candidate) => candidate.id === node.data.layerId);
  return layer !== undefined && !layer.isDefault;
}

function graphNodeInputs(node: SequenceGraphNode): GraphPort[] {
  return node.inputs.map((port) => ({ id: port.sourceName, label: port.displayName }));
}

function graphNodeOutputs(node: SequenceGraphNode): GraphPort[] {
  return node.outputs.map((port) => ({ id: port.sourceName, label: port.displayName }));
}

function nextNodeX(graph: { nodes: SequenceGraphNode[] }) {
  return (graph.nodes.reduce((max, node) => Math.max(max, node.x), 0) || 0) + 180;
}

function layerColor(index: number) {
  const colors = ["#50a0ff", "#f45b69", "#37a987", "#f6b84b", "#9b6dff", "#e86fb0"];
  return colors[index % colors.length] ?? "#50a0ff";
}
