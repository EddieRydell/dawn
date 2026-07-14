import type { ElementTarget } from "../../../types";

export function targetsEqual(left: ElementTarget, right: ElementTarget) {
  return left.kind === right.kind && left.name === right.name;
}
