export const OPEN_LAYER_GRAPH_EVENT = "dawn:open-layer-graph";

export function requestOpenLayerGraph() {
  window.dispatchEvent(new CustomEvent(OPEN_LAYER_GRAPH_EVENT));
}
