import type { LayoutTarget } from "../../../types";

export function targetsEqual(left: LayoutTarget, right: LayoutTarget) {
  return left.kind === right.kind && left.name === right.name;
}
