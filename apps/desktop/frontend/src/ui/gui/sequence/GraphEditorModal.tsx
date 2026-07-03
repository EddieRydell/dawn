import "@xyflow/react/dist/style.css";

import {
  applyNodeChanges,
  Background,
  Controls,
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  type ReactFlowInstance,
  type Connection,
  type Edge,
  type Node,
  type NodeChange,
  type NodeProps
} from "@xyflow/react";
import { Plus, Trash2, X } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type {
  SequenceEditorDocument,
  SequenceGraphClip,
  SequenceGraphNode,
  SequenceGraphOperator
} from "../../../types";

const OPERATORS: SequenceGraphOperator[] = [
  "max",
  "add",
  "multiply",
  "intensityModulate",
  "dim",
  "invert",
  "colorize",
  "delay",
  "echo",
  "remapNearest"
];

type GraphNodeData = {
  label: string;
  kind: "source" | "operator" | "output";
  inputs: GraphPort[];
  outputs: GraphPort[];
};

type GraphFlowNode = Node<GraphNodeData, "dawn">;

type GraphPort = {
  id: string;
  label: string;
};

type GraphContextMenu = {
  screenX: number;
  screenY: number;
  flowX: number;
  flowY: number;
} | null;

const GRAPH_NODE_TYPES = {
  dawn: GraphFlowNodeView
};

export function GraphEditorModal({
  document,
  clip,
  selectedNodeId,
  setSelectedNodeId,
  onClose
}: {
  document: SequenceEditorDocument;
  clip: SequenceGraphClip;
  selectedNodeId: number | null;
  setSelectedNodeId: (nodeId: number | null) => void;
  onClose: () => void;
}) {
  return (
    <div className="graph-modal-backdrop" role="presentation">
      <div className="graph-modal" role="dialog" aria-modal="true" aria-label="Effect graph editor">
        <div className="graph-modal-header">
          <div>
            <h2>Effect graph</h2>
            <span>Clip {clip.id}</span>
          </div>
          <button type="button" className="icon-button" onClick={onClose} aria-label="Close graph editor">
            <X size={16} />
          </button>
        </div>
        <GraphEditorWorkspace
          document={document}
          clip={clip}
          selectedNodeId={selectedNodeId}
          setSelectedNodeId={setSelectedNodeId}
        />
      </div>
    </div>
  );
}

export function GraphEditorWorkspace({
  document,
  clip,
  selectedNodeId,
  setSelectedNodeId
}: {
  document: SequenceEditorDocument;
  clip: SequenceGraphClip;
  selectedNodeId: number | null;
  setSelectedNodeId: (nodeId: number | null) => void;
}) {
  const [sourceScriptValue, setSourceScriptValue] = useState("0");
  const [operatorValue, setOperatorValue] = useState<SequenceGraphOperator>("max");
  const sampleScripts = document.effectScripts.filter((script) => script.kind === "sample");

  const flowNodes = useMemo(() => graphFlowNodes(clip.nodes), [clip.nodes]);
  const flowNodesKey = useMemo(() => clip.nodes.map((node) => `${node.id}:${graphNodeLabel(node)}`).join("|"), [clip.nodes]);
  const selectedNode = selectedNodeId === null ? null : clip.nodes.find((node) => node.id === selectedNodeId) ?? null;

  const edges = useMemo<Edge[]>(
    () =>
      clip.edges.map((edge) => ({
        id: `${edge.fromNode}:${edge.toNode}:${edge.fromPort}:${edge.toPort}`,
        source: String(edge.fromNode),
        target: String(edge.toNode),
        sourceHandle: edge.fromPort,
        targetHandle: edge.toPort,
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color: "#8ecae6"
        },
        className: "graph-flow-edge"
      })),
    [clip.edges]
  );

  const addSource = (x = nextNodeX(clip), y = 80) => {
    const script = sampleScripts[Number(sourceScriptValue)]?.script;
    if (script === undefined) return;
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "addGraphSourceNode",
        clipId: clip.id,
        script,
        x,
        y
      })
    );
  };

  const addOperator = (operator = operatorValue, x = nextNodeX(clip), y = 280) => {
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "addGraphOperatorNode",
        clipId: clip.id,
        operator,
        x,
        y
      })
    );
  };

  const deleteSelected = () => {
    if (selectedNodeId === null) return;
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "deleteGraphNode",
        clipId: clip.id,
        nodeId: selectedNodeId
      })
    ).then(() => {
      setSelectedNodeId(null);
    });
  };

  return (
    <div className="graph-workspace">
        <div className="graph-modal-toolbar">
          <label>
            Source
            <select
              value={sourceScriptValue}
              onChange={(event) => {
                setSourceScriptValue(event.currentTarget.value);
              }}
            >
              {sampleScripts.map((script, index) => (
                <option key={`${script.script.path}:${script.script.effectName}`} value={String(index)}>
                  {script.name}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            className="graph-toolbar-button"
            onClick={() => {
              addSource();
            }}
            disabled={sampleScripts.length === 0}
          >
            <Plus size={14} />
            Source
          </button>
          <label>
            Operator
            <select
              value={operatorValue}
              onChange={(event) => {
                setOperatorValue(event.currentTarget.value as SequenceGraphOperator);
              }}
            >
              {OPERATORS.map((operator) => (
                <option key={operator} value={operator}>
                  {operatorLabel(operator)}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            className="graph-toolbar-button"
            onClick={() => {
              addOperator();
            }}
          >
            <Plus size={14} />
            Operator
          </button>
          <button
            type="button"
            className="graph-toolbar-button danger"
            onClick={deleteSelected}
            disabled={selectedNodeId === null || selectedNode?.kind.type === "output"}
          >
            <Trash2 size={14} />
            Delete
          </button>
        </div>
        <div className="graph-modal-body">
          <div className="graph-flow-pane">
            <GraphFlowCanvas
              key={flowNodesKey}
              clipId={clip.id}
              initialNodes={flowNodes}
              edges={edges}
              setSelectedNodeId={setSelectedNodeId}
              addSourceAt={addSource}
              addOperatorAt={addOperator}
              sourceDisabled={sampleScripts.length === 0}
            />
          </div>
        </div>
    </div>
  );
}

function GraphFlowCanvas({
  clipId,
  initialNodes,
  edges,
  setSelectedNodeId,
  addSourceAt,
  addOperatorAt,
  sourceDisabled
}: {
  clipId: number;
  initialNodes: GraphFlowNode[];
  edges: Edge[];
  setSelectedNodeId: (id: number | null) => void;
  addSourceAt: (x: number, y: number) => void;
  addOperatorAt: (operator: SequenceGraphOperator, x: number, y: number) => void;
  sourceDisabled: boolean;
}) {
  const [nodes, setNodes] = useState(initialNodes);
  const [contextMenu, setContextMenu] = useState<GraphContextMenu>(null);
  const flow = useRef<ReactFlowInstance<GraphFlowNode> | null>(null);
  const handleNodesChange = useCallback(
    (changes: NodeChange<GraphFlowNode>[]) => {
      setNodes((current) => applyNodeChanges(changes, current));
      persistNodePositions(clipId, changes);
    },
    [clipId]
  );

  const closeContextMenu = () => {
    setContextMenu(null);
  };

  return (
    <>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={GRAPH_NODE_TYPES}
        fitView
        onInit={(instance) => {
          flow.current = instance;
        }}
        onNodesChange={handleNodesChange}
        onNodeClick={(_, node) => {
          closeContextMenu();
          setSelectedNodeId(Number(node.id));
        }}
        onPaneClick={() => {
          closeContextMenu();
          setSelectedNodeId(null);
        }}
        onPaneContextMenu={(event) => {
          event.preventDefault();
          const position = flow.current?.screenToFlowPosition({ x: event.clientX, y: event.clientY }) ?? { x: 0, y: 0 };
          setContextMenu({
            screenX: event.clientX,
            screenY: event.clientY,
            flowX: position.x,
            flowY: position.y
          });
        }}
        onConnect={(connection) => {
          closeContextMenu();
          connectNodes(clipId, connection);
        }}
        onEdgesDelete={(deleted) => {
          closeContextMenu();
          disconnectEdges(clipId, deleted);
        }}
        defaultEdgeOptions={{
          type: "smoothstep",
          className: "graph-flow-edge"
        }}
      >
        <Background color="#2c3036" gap={32} />
        <Controls className="graph-flow-controls" />
      </ReactFlow>
      {contextMenu !== null && (
        <div className="graph-context-menu" style={{ left: contextMenu.screenX, top: contextMenu.screenY }}>
          <button
            type="button"
            disabled={sourceDisabled}
            onClick={() => {
              addSourceAt(contextMenu.flowX, contextMenu.flowY);
              closeContextMenu();
            }}
          >
            Source
          </button>
          <div className="graph-context-menu-divider" />
          {OPERATORS.map((operator) => (
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
          ))}
        </div>
      )}
    </>
  );
}

function GraphFlowNodeView({ data, selected }: NodeProps<GraphFlowNode>) {
  return (
    <div className={`graph-flow-node-card ${data.kind} ${selected ? "selected" : ""}`}>
      <div className="graph-flow-node-title">{data.label}</div>
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

function persistNodePositions(clipId: number, changes: NodeChange<GraphFlowNode>[]) {
  for (const change of changes) {
    const isDragging = Boolean((change as { dragging?: boolean }).dragging);
    if (change.type !== "position" || change.position === undefined || isDragging) continue;
    const position = change.position;
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "moveGraphNode",
        clipId,
        nodeId: Number(change.id),
        x: position.x,
        y: position.y
      })
    );
  }
}

function graphFlowNodes(nodes: SequenceGraphNode[]): GraphFlowNode[] {
  return nodes.map((node) => ({
    id: String(node.id),
    type: "dawn",
    position: { x: node.x, y: node.y },
    sourcePosition: Position.Right,
    targetPosition: Position.Left,
    data: {
      label: graphNodeLabel(node),
      kind: node.kind.type,
      inputs: graphNodeInputs(node),
      outputs: graphNodeOutputs(node)
    }
  }));
}

function connectNodes(clipId: number, connection: Connection) {
  const fromNode = Number(connection.source);
  const toNode = Number(connection.target);
  const fromPort = connection.sourceHandle;
  const toPort = connection.targetHandle;
  if (!Number.isFinite(fromNode) || !Number.isFinite(toNode) || fromPort === null || toPort === null) return;
  void runGuiEditCommand(() =>
    commands.applySequenceGuiEdit({
      type: "connectGraphNodes",
      clipId,
      fromNode,
      fromPort,
      toNode,
      toPort
    })
  );
}

function disconnectEdges(clipId: number, edges: Edge[]) {
  for (const edge of edges) {
    const fromNode = Number(edge.source);
    const toNode = Number(edge.target);
    const fromPort = edge.sourceHandle;
    const toPort = edge.targetHandle;
    if (
      !Number.isFinite(fromNode) ||
      !Number.isFinite(toNode) ||
      fromPort === null ||
      fromPort === undefined ||
      toPort === null ||
      toPort === undefined
    ) {
      continue;
    }
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "disconnectGraphNodes",
        clipId,
        fromNode,
        fromPort,
        toNode,
        toPort
      })
    );
  }
}

function graphNodeLabel(node: SequenceGraphNode) {
  if (node.kind.type === "source") return node.kind.script;
  if (node.kind.type === "operator") return operatorLabel(node.kind.operator);
  return "Output";
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

function nextNodeX(clip: SequenceGraphClip) {
  return (clip.nodes.reduce((max, node) => Math.max(max, node.x), 0) || 0) + 180;
}
