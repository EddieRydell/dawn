import type { LayoutTargetDto } from "../../../bindings";

export function targetsEqual(left: LayoutTargetDto, right: LayoutTargetDto) {
  return left.kind === right.kind && left.name === right.name;
}
