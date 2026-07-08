import "@xyflow/react/dist/style.css";

import {
  Background,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  applyNodeChanges,
  type ReactFlowInstance,
  type Connection,
  type Edge,
  type Node,
  type NodeChange,
  type NodeProps
} from "@xyflow/react";
import { X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type {
  SequenceEditorDocument,
  SequenceGraphNode,
  SequenceGraphOperator
} from "../../../types";
import {
  GRAPH_NEUTRAL_EDGE_COLOR,
  graphEdgeId,
  graphEdgeLineages,
  parseGraphEdgeId,
  type GraphEdgeIdParts
} from "./graphEdge";

const OPERATORS: SequenceGraphOperator[] = [
  "max",
  "add",
  "multiply",
  "intensityModulate",
  "dim",
  "invert",
  "colorize",
  "delay",
  "echo"
];

type GraphNodeData = {
  label: string;
  kind: "layer" | "operator" | "output";
  color: string | null;
  inputs: GraphPort[];
  outputs: GraphPort[];
};

type GraphFlowNode = Node<GraphNodeData, "dawn">;

type GraphPort = {
  id: string;
  label: string;
};

type SelectedGraphItem = { type: "node"; id: string } | { type: "edge"; id: string } | null;

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

export function GraphEditorWorkspace({
  document,
  selectedItem,
  setSelectedItem
}: {
  document: SequenceEditorDocument;
  selectedItem: SelectedGraphItem;
  setSelectedItem: (item: SelectedGraphItem) => void;
}) {
  const graph = document.compositionGraph;
  const selectedNodeId = selectedItem?.type === "node" ? selectedItem.id : null;
  const selectedEdgeId = selectedItem?.type === "edge" ? selectedItem.id : null;
  const flowNodes = useMemo(() => graphFlowNodes(graph.nodes, selectedNodeId), [graph.nodes, selectedNodeId]);
  const edgeLineages = useMemo(() => graphEdgeLineages(graph), [graph]);

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
          type: "default",
          selected: id === selectedEdgeId,
          interactionWidth: 18,
          markerEnd: {
            type: MarkerType.ArrowClosed,
            color
          },
          style: {
            stroke: color,
            strokeWidth: id === selectedEdgeId ? 3 : 2
          },
          className: id === selectedEdgeId ? "graph-flow-edge selected" : "graph-flow-edge"
        };
      }),
    [edgeLineages, graph.edges, selectedEdgeId]
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
          />
        </div>
      </div>
    </div>
  );
}

function GraphFlowCanvas({
  nodes,
  edges,
  selectedItem,
  setSelectedItem,
  addOperatorAt
}: {
  nodes: GraphFlowNode[];
  edges: Edge[];
  selectedItem: SelectedGraphItem;
  setSelectedItem: (item: SelectedGraphItem) => void;
  addOperatorAt: (operator: SequenceGraphOperator, x: number, y: number) => void;
}) {
  const [flowNodes, setFlowNodes] = useState<GraphFlowNode[]>(nodes);
  const [contextMenu, setContextMenu] = useState<GraphContextMenu>(null);
  const flow = useRef<ReactFlowInstance<GraphFlowNode> | null>(null);
  const pendingPositions = useRef<Map<string, { x: number; y: number }>>(new Map());

  useEffect(() => {
    let cancelled = false;
    queueMicrotask(() => {
      if (cancelled) return;
      setFlowNodes((current) => mergeGraphNodes(nodes, current, pendingPositions.current));
    });
    return () => {
      cancelled = true;
    };
  }, [nodes]);

  const handleNodesChange = useCallback((changes: NodeChange<GraphFlowNode>[]) => {
    setFlowNodes((current) => applyNodeChanges(changes, current));
    persistNodePositions(changes, pendingPositions.current);
  }, []);

  const closeContextMenu = () => {
    setContextMenu(null);
  };

  const deleteEdgeById = useCallback(
    (edgeId: string) => {
      const edge = edges.find((candidate) => candidate.id === edgeId);
      if (edge === undefined) return;
      void deleteFlowEdge(edge).then(() => {
        setSelectedItem(null);
      });
    },
    [edges, setSelectedItem]
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

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Delete" && event.key !== "Backspace") return;
      if (selectedItem?.type !== "edge") return;
      event.preventDefault();
      deleteEdgeById(selectedItem.id);
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [deleteEdgeById, selectedItem]);

  return (
    <>
      <ReactFlow
        nodes={flowNodes}
        edges={edges}
        nodeTypes={GRAPH_NODE_TYPES}
        fitView
        minZoom={0.2}
        maxZoom={2}
        deleteKeyCode={null}
        defaultEdgeOptions={{
          type: "default",
          interactionWidth: 18,
          className: "graph-flow-edge"
        }}
        onInit={(instance) => {
          flow.current = instance;
        }}
        onNodesChange={handleNodesChange}
        onNodeClick={(_, node) => {
          closeContextMenu();
          setSelectedItem({ type: "node", id: node.id });
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
        onEdgeClick={(_, edge) => {
          closeContextMenu();
          setSelectedItem({ type: "edge", id: edge.id });
        }}
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
        onPaneClick={() => {
          closeContextMenu();
          setSelectedItem(null);
        }}
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
              disabled={flowNodes.find((node) => node.id === contextMenu.nodeId)?.data.kind !== "operator"}
              onClick={() => {
                deleteNodeById(contextMenu.nodeId);
                closeContextMenu();
              }}
            >
              Delete operator
            </button>
          ) : contextMenu.type === "edge" ? (
            <button
              type="button"
              onClick={() => {
                deleteEdgeById(contextMenu.edgeId);
                closeContextMenu();
              }}
            >
              Delete connection
            </button>
          ) : (
            OPERATORS.map((operator) => (
              <button
                key={operator}
                type="button"
                onClick={() => {
                  addOperatorAt(operator, contextMenu.flowX, contextMenu.flowY);
                  closeContextMenu();
                }}
              >
                {operatorLabel(operator)}
              </button>
            ))
          )}
        </div>
      )}
    </>
  );
}

function GraphFlowNodeView({ data, selected }: NodeProps<GraphFlowNode>) {
  return (
    <div
      className={`graph-flow-node-card ${data.kind} ${selected ? "selected" : ""}`}
      style={data.color === null ? undefined : { borderLeftColor: data.color }}
    >
      <div className="graph-flow-node-title">
        {data.color !== null && <span className="graph-flow-node-swatch" style={{ background: data.color }} />}
        <span>{data.label}</span>
      </div>
      <div className="graph-flow-node-body">
        {data.inputs.map((port, index) => (
          <Handle
            key={port.id}
            id={port.id}
            type="target"
            position={Position.Left}
            className="graph-flow-handle input"
            style={portStyle(index, data.inputs.length)}
            aria-label={`Input ${port.label}`}
            title={port.label}
          />
        ))}
        {data.outputs.map((port, index) => (
          <Handle
            key={port.id}
            id={port.id}
            type="source"
            position={Position.Right}
            className="graph-flow-handle output"
            style={portStyle(index, data.outputs.length)}
            aria-label={`Output ${port.label}`}
            title={port.label}
          />
        ))}
      </div>
    </div>
  );
}

function portStyle(index: number, count: number) {
  return {
    top: `${((index + 1) * 100) / (count + 1)}%`
  };
}

function samePosition(left: { x: number; y: number }, right: { x: number; y: number }) {
  return Math.abs(left.x - right.x) < 0.001 && Math.abs(left.y - right.y) < 0.001;
}

function mergeGraphNodes(
  sourceNodes: GraphFlowNode[],
  currentNodes: GraphFlowNode[],
  pendingPositions: Map<string, { x: number; y: number }>
) {
  const currentById = new Map(currentNodes.map((node) => [node.id, node]));
  return sourceNodes.map((sourceNode) => {
    const currentNode = currentById.get(sourceNode.id);
    const pending = pendingPositions.get(sourceNode.id);
    if (pending !== undefined) {
      if (samePosition(pending, sourceNode.position)) {
        pendingPositions.delete(sourceNode.id);
        return sourceNode;
      }
      return { ...sourceNode, position: pending };
    }
    if (currentNode?.dragging === true) {
      return { ...sourceNode, position: currentNode.position, dragging: true };
    }
    return sourceNode;
  });
}

function persistNodePositions(changes: NodeChange<GraphFlowNode>[], pendingPositions: Map<string, { x: number; y: number }>) {
  for (const change of changes) {
    const isDragging = Boolean((change as { dragging?: boolean }).dragging);
    if (change.type !== "position" || change.position === undefined || isDragging) continue;
    const position = change.position;
    pendingPositions.set(change.id, position);
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "moveGraphNode",
        nodeId: change.id,
        x: position.x,
        y: position.y
      })
    );
  }
}

function graphFlowNodes(nodes: SequenceGraphNode[], selectedNodeId: string | null): GraphFlowNode[] {
  return nodes.map((node) => ({
    id: node.id,
    type: "dawn",
    position: { x: node.x, y: node.y },
    selected: node.id === selectedNodeId,
    sourcePosition: Position.Right,
    targetPosition: Position.Left,
    data: {
      label: graphNodeLabel(node),
      kind: node.kind.type,
      color: graphNodeColor(node),
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

function graphNodeLabel(node: SequenceGraphNode) {
  if (node.kind.type === "layer") return node.kind.layerName;
  if (node.kind.type === "operator") return operatorLabel(node.kind.operator);
  return "Output";
}

function graphNodeColor(node: SequenceGraphNode) {
  return node.kind.type === "layer" ? node.kind.layerColor : null;
}

function graphNodeInputs(node: SequenceGraphNode): GraphPort[] {
  if (node.kind.type === "output") return [{ id: "input", label: "Input" }];
  if (node.kind.type !== "operator") return [];
  if (isBinaryOperator(node.kind.operator)) {
    return [
      { id: "a", label: "A" },
      { id: "b", label: "B" }
    ];
  }
  return [{ id: "input", label: "Input" }];
}

function graphNodeOutputs(node: SequenceGraphNode): GraphPort[] {
  return node.kind.type === "output" ? [] : [{ id: "output", label: "Output" }];
}

function isBinaryOperator(operator: SequenceGraphOperator) {
  return operator === "max" || operator === "add" || operator === "multiply" || operator === "intensityModulate";
}

function operatorLabel(operator: SequenceGraphOperator) {
  return operator.replace(/[A-Z]/g, (match) => ` ${match}`).replace(/^./, (match) => match.toUpperCase());
}

function nextNodeX(graph: { nodes: SequenceGraphNode[] }) {
  return (graph.nodes.reduce((max, node) => Math.max(max, node.x), 0) || 0) + 180;
}
